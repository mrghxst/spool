// writer.worker: positional writes into the job folder. Segments arrive
// straight from the net workers; files stay open until the coordinator
// closes them, so a whole file is never held in memory.

import { openJobDir, openWriter, rename, type PositionalWriter } from '../fs';
import type { Target } from '../types';
import type { FromWriter, ToWriter } from './protocol';

declare const self: DedicatedWorkerGlobalScope;

let coord: MessagePort | null = null;
let target: Target | null = null;
let dir: FileSystemDirectoryHandle | null = null;
let names = new Map<number, string>();
const open = new Map<number, Promise<PositionalWriter>>();
/** Pending work per file, so close waits for every write. */
const pending = new Map<number, Promise<void>>();
/**
 * Files whose close was requested. A file is closed only when every article
 * is in, so a later write is a duplicate (from a connection that was being
 * replaced) and is dropped rather than reopening the file.
 */
const closing = new Set<number>();

/**
 * Articles are collected per file and written in large contiguous blocks.
 * Every write call into a picked folder has a fixed cost (a round trip to
 * the browser, and often a virus scan of the swap file on Windows), so one
 * call per 700 KB article can be far slower than the disk.
 */
const BATCH_BYTES = 16 * 1024 * 1024;
const BATCH_MS = 250;
type Piece = { segId: number; offset: number; data: Uint8Array };
type Batch = { pieces: Piece[]; bytes: number; timer: ReturnType<typeof setTimeout> | null };
const batches = new Map<number, Batch>();

function add(fileIdx: number, piece: Piece) {
  let b = batches.get(fileIdx);
  if (!b) {
    b = { pieces: [], bytes: 0, timer: null };
    batches.set(fileIdx, b);
  }
  b.pieces.push(piece);
  b.bytes += piece.data.byteLength;
  if (b.bytes >= BATCH_BYTES) flush(fileIdx);
  else b.timer ??= setTimeout(() => flush(fileIdx), BATCH_MS);
}

/** Queues the file's collected articles as one write per contiguous run. */
function flush(fileIdx: number) {
  const b = batches.get(fileIdx);
  if (!b) return;
  batches.delete(fileIdx);
  if (b.timer) clearTimeout(b.timer);
  const pieces = b.pieces.sort((x, y) => x.offset - y.offset);
  void enqueue(fileIdx, async () => {
    const w = await writerFor(fileIdx);
    let i = 0;
    while (i < pieces.length) {
      let j = i + 1;
      let end = pieces[i].offset + pieces[i].data.byteLength;
      while (j < pieces.length && pieces[j].offset === end) end += pieces[j++].data.byteLength;
      if (j - i === 1) {
        await w.write(pieces[i].offset, pieces[i].data);
      } else {
        const run = new Uint8Array(end - pieces[i].offset);
        for (let k = i; k < j; k++) run.set(pieces[k].data, pieces[k].offset - pieces[i].offset);
        await w.write(pieces[i].offset, run);
      }
      i = j;
    }
    for (const p of pieces) send({ type: 'written', segId: p.segId, fileIdx, bytes: p.data.byteLength });
  });
}

function dropBatches() {
  for (const b of batches.values()) if (b.timer) clearTimeout(b.timer);
  batches.clear();
}
let failed = false;

function send(msg: FromWriter) {
  coord?.postMessage(msg);
}

function fail(e: unknown) {
  if (failed) return;
  failed = true;
  const name = (e as DOMException)?.name;
  const message =
    name === 'QuotaExceededError'
      ? 'The disk is full or the browser storage quota is used up.'
      : name === 'NotAllowedError'
        ? 'Spool lost permission to write to the download folder.'
        : `Couldn't write to disk: ${(e as Error)?.message ?? e}`;
  send({ type: 'error', message });
}

function writerFor(fileIdx: number): Promise<PositionalWriter> {
  let w = open.get(fileIdx);
  if (!w) {
    const name = names.get(fileIdx) ?? `file-${fileIdx}`;
    w = openWriter(dir!, name, target!.kind);
    open.set(fileIdx, w);
  }
  return w;
}

function enqueue(fileIdx: number, f: () => Promise<void>) {
  const next = (pending.get(fileIdx) ?? Promise.resolve()).then(f).catch(fail);
  pending.set(fileIdx, next);
  return next;
}

async function handle(msg: ToWriter) {
  switch (msg.type) {
    case 'job': {
      failed = false;
      open.clear();
      pending.clear();
      closing.clear();
      dropBatches();
      target = msg.target;
      names = new Map(msg.files.map((f) => [f.idx, f.name]));
      dir = await openJobDir(msg.target, msg.folder);
      send({ type: 'ready' });
      break;
    }
    case 'rename-pending': {
      // A file not opened yet: just change the name it will be created with.
      if (!open.has(msg.fileIdx)) names.set(msg.fileIdx, msg.name);
      break;
    }
    case 'write': {
      if (!dir) return;
      const { fileIdx, segId, offset, data } = msg;
      if (closing.has(fileIdx)) return;
      add(fileIdx, { segId, offset, data: new Uint8Array(data) });
      break;
    }
    case 'close-file': {
      const { fileIdx, size, rename: newName } = msg;
      closing.add(fileIdx);
      flush(fileIdx);
      void enqueue(fileIdx, async () => {
        const w = await writerFor(fileIdx);
        if (size !== null) await w.truncate(size);
        await w.close();
        open.delete(fileIdx);
        let name = names.get(fileIdx)!;
        if (newName && newName !== name) {
          try {
            await rename(dir!, name, newName);
            name = newName;
            names.set(fileIdx, name);
          } catch {
            // Keep the original name.
          }
        }
        send({ type: 'file-closed', fileIdx, name });
      });
      break;
    }
    case 'flush': {
      for (const fileIdx of [...batches.keys()]) flush(fileIdx);
      await Promise.all(pending.values());
      send({ type: 'flushed' });
      break;
    }
    case 'abort': {
      dropBatches();
      await Promise.allSettled(pending.values());
      for (const w of open.values()) {
        await w.then((x) => x.close()).catch(() => {});
      }
      open.clear();
      pending.clear();
      dir = null;
      break;
    }
  }
}

function listen(port: MessagePort) {
  port.onmessage = (e: MessageEvent<ToWriter>) => {
    handle(e.data).catch(fail);
  };
}

self.onmessage = (e: MessageEvent<{ type: 'wire' }>) => {
  if (e.data?.type !== 'wire') return;
  const [c, ...nets] = e.ports;
  coord = c;
  listen(c);
  nets.forEach(listen);
};
