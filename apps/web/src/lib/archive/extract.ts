// Archive extraction with libarchive compiled to WebAssembly. Loaded only
// when a job has archives. Volumes are mounted with WORKERFS, so libarchive
// reads them from disk-backed File objects; output is streamed to disk in
// 1 MB chunks. Nothing is held in memory whole.

import { sameFolder, shortName } from '../core/names';
import { archiveSets, type ArchiveSet } from '../core/plan';
import { getFile, openWriter, remove } from '../fs';
import type { Target } from '../types';
import createArchiveModule, { type ArchiveModule } from './vendor/archive.mjs';
import wasmUrl from './vendor/archive.wasm?url';

export type ExtractResult = {
  ok: boolean;
  extractedSets: number;
  /** Archive parts that were fully extracted (safe to delete). */
  consumed: string[];
  messages: string[];
  encrypted: boolean;
};

const CHUNK = 1 << 20;

let modPromise: Promise<ArchiveModule> | null = null;

function load(): Promise<ArchiveModule> {
  modPromise ??= createArchiveModule({
    locateFile: () => wasmUrl,
    print: () => {},
    printErr: () => {},
  });
  return modPromise;
}

/** Splits an entry path into safe segments, or null if it escapes the folder. */
export function safePath(path: string): string[] | null {
  const parts = path
    .replace(/\\/g, '/')
    .split('/')
    .filter((p) => p !== '' && p !== '.');
  if (parts.length === 0 || parts.some((p) => p === '..')) return null;
  const clean = parts.map((p) =>
    p
      .replace(/[<>:"|?*\u0000-\u001f]/g, '_')
      .replace(/[. ]+$/, '')
      .slice(0, 200),
  );
  return clean.some((p) => p === '') ? null : clean;
}

async function subdir(root: FileSystemDirectoryHandle, parts: string[]): Promise<FileSystemDirectoryHandle> {
  let d = root;
  for (const p of parts) d = await d.getDirectoryHandle(p, { create: true });
  return d;
}

const ENCRYPTED = /encrypt|passphrase|password/i;

const PATH_TOO_LONG =
  'Windows refused a path longer than 260 characters. Choose a download folder with a shorter path, such as C:\\Downloads.';

type Out = { w: Awaited<ReturnType<typeof openWriter>>; parent: FileSystemDirectoryHandle; name: string };

/**
 * Opens a writer for an extracted file. When Windows refuses the path
 * (NotFoundError for paths over 260 characters), retries with a shorter
 * name, then with the short name in the job folder itself.
 */
async function openOut(
  dir: FileSystemDirectoryHandle,
  parent: FileSystemDirectoryHandle,
  name: string,
  kind: Target['kind'],
): Promise<Out> {
  const tries: [FileSystemDirectoryHandle, string][] = [
    [parent, name],
    [parent, shortName(name)],
    [dir, shortName(name)],
  ];
  let last: unknown;
  for (const [p, n] of tries) {
    try {
      return { w: await openWriter(p, n, kind), parent: p, name: n };
    } catch (e) {
      last = e;
      if ((e as DOMException)?.name !== 'NotFoundError') throw e;
      await remove(p, n);
    }
  }
  throw (last as DOMException)?.name === 'NotFoundError' ? new Error(PATH_TOO_LONG) : last;
}

type SetOutcome = { ok: true; files: number } | { ok: false; encrypted: boolean; message: string };

async function extractSet(
  M: ArchiveModule,
  dir: FileSystemDirectoryHandle,
  kind: Target['kind'],
  set: ArchiveSet,
  password: string | null,
  folder: string,
  mount: string,
  progress: (bytes: number) => void,
): Promise<SetOutcome> {
  const files = await Promise.all(set.parts.map((p) => getFile(dir, p)));
  M.FS.mkdir(mount);
  M.FS.mount(M.FS.filesystems.WORKERFS, { files }, mount);
  const ptrs = set.parts.map((p) => M.stringToNewUTF8(`${mount}/${p}`));
  const list = M._malloc(ptrs.length * 4);
  new Uint32Array(M.HEAPU8.buffer, list, ptrs.length).set(ptrs);
  const pw = password ? M.stringToNewUTF8(password) : 0;
  const buf = M._malloc(CHUNK);
  const written: string[][] = [];
  // On failure, remove what this set wrote so only complete files remain.
  const undo = async (o: SetOutcome): Promise<SetOutcome> => {
    for (const path of written) {
      try {
        const parent = await subdir(dir, path.slice(0, -1));
        await remove(parent, path[path.length - 1]);
      } catch {
        // Already gone.
      }
    }
    return o;
  };
  try {
    if (M._spool_open(list, ptrs.length, pw) !== 0) {
      const err = M.UTF8ToString(M._spool_error());
      return undo({ ok: false, encrypted: ENCRYPTED.test(err), message: err || 'unrecognised archive' });
    }
    let count = 0;
    for (;;) {
      const r = M._spool_next();
      if (r === 0) break;
      if (r < 0) {
        const err = M.UTF8ToString(M._spool_error());
        return undo({ ok: false, encrypted: ENCRYPTED.test(err), message: err || 'damaged archive' });
      }
      const type = M._spool_entry_type();
      let path = safePath(M.UTF8ToString(M._spool_entry_path()));
      // Drop a top folder named like the job folder: it only lengthens paths.
      if (path && sameFolder(path[0], folder)) path = path.length > 1 ? path.slice(1) : null;
      if (type !== 1 || !path) {
        if (type === 2 && path) await subdir(dir, path).catch(() => {});
        M._spool_skip();
        continue;
      }
      const encrypted = M._spool_entry_encrypted() === 1;
      if (path.length === 1 && set.parts.includes(path[0])) {
        M._spool_skip();
        continue;
      }
      let parent: FileSystemDirectoryHandle;
      try {
        parent = await subdir(dir, path.slice(0, -1));
      } catch (e) {
        if ((e as DOMException)?.name !== 'NotFoundError') throw e;
        parent = dir;
      }
      const out = await openOut(dir, parent, path[path.length - 1], kind);
      const w = out.w;
      written.push(out.parent === dir ? [out.name] : [...path.slice(0, -1), out.name]);
      let offset = 0;
      let failed: string | null = null;
      for (;;) {
        const n = M._spool_read(buf, CHUNK);
        if (n === 0) break;
        if (n < 0) {
          failed = M.UTF8ToString(M._spool_error()) || 'read error';
          break;
        }
        // Copy out: the heap can move if memory grows.
        await w.write(offset, M.HEAPU8.slice(buf, buf + n));
        offset += n;
        progress(n);
      }
      await w.truncate(offset);
      await w.close();
      if (failed) {
        return undo({ ok: false, encrypted: encrypted || ENCRYPTED.test(failed), message: failed });
      }
      count++;
    }
    return { ok: true, files: count };
  } finally {
    M._spool_close();
    M._free(buf);
    M._free(list);
    ptrs.forEach((p) => M._free(p));
    if (pw) M._free(pw);
    try {
      M.FS.unmount(mount);
      M.FS.rmdir(mount);
    } catch {
      // Ignore.
    }
  }
}

export async function extractAll(
  dir: FileSystemDirectoryHandle,
  kind: Target['kind'],
  names: string[],
  password: string | null,
  onProgress: (done: number, total: number, detail: string) => void,
  folder = '',
): Promise<ExtractResult> {
  const sets = archiveSets(names);
  const result: ExtractResult = { ok: true, extractedSets: 0, consumed: [], messages: [], encrypted: false };
  if (sets.length === 0) return result;
  const M = await load();
  let total = 0;
  for (const s of sets) {
    for (const p of s.parts) total += (await getFile(dir, p)).size;
  }
  let done = 0;
  let n = 0;
  for (const set of sets) {
    onProgress(done, total, set.first);
    const outcome = await extractSet(M, dir, kind, set, password, folder, `/v${n++}`, (bytes) => {
      done += bytes;
      onProgress(Math.min(done, total), total, set.first);
    });
    if (outcome.ok) {
      result.extractedSets++;
      result.consumed.push(...set.parts);
      continue;
    }
    result.ok = false;
    if (outcome.encrypted) {
      result.encrypted = true;
      result.messages.push(
        `${set.first} is password-protected and can't be extracted in the browser, so its parts were kept as downloaded.`,
      );
    } else {
      result.messages.push(`Couldn't extract ${set.first}: ${outcome.message}. Its parts were kept as downloaded.`);
    }
  }
  onProgress(total, total, '');
  return result;
}
