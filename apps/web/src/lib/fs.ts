// Storage for job files, used inside workers. Two back ends:
//  - fsa:  a folder the user picked (File System Access, Chromium)
//  - opfs: the origin private file system, saved out at the end (fallback)

import type { Target } from './types';

export async function opfsJobsRoot(): Promise<FileSystemDirectoryHandle> {
  const root = await navigator.storage.getDirectory();
  return root.getDirectoryHandle('jobs', { create: true });
}

export async function openJobDir(target: Target, folder: string): Promise<FileSystemDirectoryHandle> {
  const root = target.kind === 'fsa' ? target.dir : await opfsJobsRoot();
  return root.getDirectoryHandle(folder, { create: true });
}

export interface PositionalWriter {
  write(offset: number, data: Uint8Array): Promise<void>;
  truncate(size: number): Promise<void>;
  close(): Promise<void>;
}

type SyncHandle = {
  write(data: BufferSource, opts: { at: number }): number;
  truncate(size: number): void;
  flush(): void;
  close(): void;
  getSize(): number;
};

/** Opens `name` for positional writes without loading it into memory. */
export async function openWriter(
  dir: FileSystemDirectoryHandle,
  name: string,
  kind: Target['kind'],
): Promise<PositionalWriter> {
  const fh = await dir.getFileHandle(name, { create: true });
  if (kind === 'opfs') {
    const h = (await (
      fh as unknown as { createSyncAccessHandle(): Promise<SyncHandle> }
    ).createSyncAccessHandle()) as SyncHandle;
    return {
      async write(offset, data) {
        let done = 0;
        while (done < data.length) {
          done += h.write(data.subarray(done) as Uint8Array<ArrayBuffer>, { at: offset + done });
        }
      },
      async truncate(size) {
        h.truncate(size);
      },
      async close() {
        h.flush();
        h.close();
      },
    };
  }
  const w = await fh.createWritable({ keepExistingData: true });
  // Writes to one stream must not overlap; chain them.
  let chain: Promise<void> = Promise.resolve();
  const run = (f: () => Promise<void>) => (chain = chain.then(f));
  return {
    write: (offset, data) =>
      run(() => w.write({ type: 'write', position: offset, data: data as Uint8Array<ArrayBuffer> })),
    truncate: (size) => run(() => w.truncate(size)),
    close: () => run(() => w.close()),
  };
}

export async function getFile(dir: FileSystemDirectoryHandle, name: string): Promise<File> {
  const fh = await dir.getFileHandle(name);
  return fh.getFile();
}

export async function exists(dir: FileSystemDirectoryHandle, name: string): Promise<boolean> {
  try {
    await dir.getFileHandle(name);
    return true;
  } catch {
    return false;
  }
}

export async function remove(dir: FileSystemDirectoryHandle, name: string): Promise<void> {
  try {
    await dir.removeEntry(name);
  } catch {
    // Already gone.
  }
}

export async function list(dir: FileSystemDirectoryHandle): Promise<string[]> {
  const out: string[] = [];
  for await (const [name, handle] of (
    dir as unknown as { entries(): AsyncIterable<[string, FileSystemHandle]> }
  ).entries()) {
    if (handle.kind === 'file') out.push(name);
  }
  return out.sort();
}

/** Renames a file, using move() where supported and copying otherwise. */
export async function rename(dir: FileSystemDirectoryHandle, from: string, to: string): Promise<void> {
  if (from === to) return;
  const fh = await dir.getFileHandle(from);
  const movable = fh as unknown as { move?: (name: string) => Promise<void> };
  if (typeof movable.move === 'function') {
    try {
      await remove(dir, to);
      await movable.move(to);
      return;
    } catch {
      // Fall through to copy.
    }
  }
  const src = await fh.getFile();
  const dst = await dir.getFileHandle(to, { create: true });
  const w = await dst.createWritable();
  await src.stream().pipeTo(w);
  await remove(dir, from);
}

/** Reads `[start, end)` of a file. */
export async function readRange(file: Blob, start: number, end: number): Promise<Uint8Array> {
  return new Uint8Array(await file.slice(start, end).arrayBuffer());
}

/** Clears every job staged in OPFS. */
export async function clearOpfs(): Promise<void> {
  const root = await navigator.storage.getDirectory();
  for await (const [name] of (
    root as unknown as { entries(): AsyncIterable<[string, FileSystemHandle]> }
  ).entries()) {
    await root.removeEntry(name, { recursive: true }).catch(() => {});
  }
}
