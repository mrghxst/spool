// post.worker: PAR2 verification, deobfuscation and repair (Rust/WASM),
// then extraction (libarchive/WASM, loaded only when needed) and cleanup.

import { loadEngine, type Engine } from '../engine/load';
import { exists, getFile, list, openJobDir, openWriter, readRange, remove, rename } from '../fs';
import type { Target } from '../types';
import type { FromPost, ToPost, VerifyResult } from './protocol';

declare const self: DedicatedWorkerGlobalScope;

const CHUNK = 4 * 1024 * 1024;
/** Memory for repair accumulators; larger repairs run in stripes. */
const REPAIR_BUDGET = 256 * 1024 * 1024;

type Par2Info = {
  setId: string;
  sliceSize: number;
  totalSlices: number;
  missingDescriptions: number;
  files: {
    name: string;
    id: string;
    md5: string;
    md5_16k: string;
    length: number;
    firstSlice: number;
    sliceCount: number;
    hasChecks: boolean;
  }[];
};

type Par2Set = ReturnType<Engine['Par2Set']['parse']>;

let coord: MessagePort | null = null;
let testCa: Uint8Array | null = null;

/** State kept from verify for repair. */
let state: {
  set: Par2Set;
  info: Par2Info;
  /** Global slice indexes to rebuild. */
  missing: number[];
} | null = null;

function send(msg: FromPost) {
  coord?.postMessage(msg);
}

function progress(phase: 'verifying' | 'repairing' | 'extracting', done: number, total: number, detail: string) {
  send({ type: 'post-progress', phase, done, total, detail });
}

async function md5_16k(engine: Engine, file: File): Promise<Uint8Array> {
  return engine.md5(await readRange(file, 0, Math.min(16384, file.size)));
}

async function verifyFile(
  engine: Engine,
  set: Par2Set,
  index: number,
  file: File,
  deep: boolean,
  onBytes: (n: number) => void,
): Promise<{ ok: boolean; damaged: number[] }> {
  const v = new engine.Par2Verifier(set, index, deep);
  for (let off = 0; off < file.size; off += CHUNK) {
    const chunk = await readRange(file, off, Math.min(file.size, off + CHUNK));
    v.update(chunk);
    onBytes(chunk.length);
  }
  return JSON.parse(v.finish()) as { ok: boolean; damaged: number[] };
}

async function verify(
  target: Target,
  folder: string,
  par2: string,
  files: string[],
  skipped: string[],
): Promise<VerifyResult> {
  const engine = await loadEngine(testCa);
  const dir = await openJobDir(target, folder);
  const base: VerifyResult = {
    type: 'verified',
    ok: true,
    need: 0,
    sliceSize: 0,
    renames: [],
    names: [],
    damagedFiles: [],
  };
  let set: Par2Set;
  try {
    set = engine.Par2Set.parse(await readRange(await getFile(dir, par2), 0, Infinity));
  } catch (e) {
    return { ...base, error: `the PAR2 file is unreadable (${(e as Error).message})` };
  }
  const info = JSON.parse(set.info()) as Par2Info;
  base.sliceSize = info.sliceSize;
  base.names = info.files.map((f) => f.name);
  const setNames = new Set(info.files.map((f) => f.name));

  // Deobfuscation: match files to the PAR2 set by their first 16 KiB, or
  // failing that (the first article may be missing) by exact length.
  const present = new Set(await list(dir));
  const adopt = async (name: string, real: string) => {
    if (present.has(real) || real.includes('/') || real.includes('\\')) return false;
    try {
      await rename(dir, name, real);
    } catch {
      return false;
    }
    present.delete(name);
    present.add(real);
    base.renames.push([name, real]);
    return true;
  };
  const unmatched: { name: string; size: number }[] = [];
  for (const name of files) {
    if (setNames.has(name) || !present.has(name)) continue;
    const file = await getFile(dir, name);
    const i = set.match_16k(await md5_16k(engine, file), file.size);
    if (i < 0 || !(await adopt(name, info.files[i].name))) unmatched.push({ name, size: file.size });
  }
  for (const u of unmatched) {
    const candidates = info.files.filter((f) => !present.has(f.name) && f.length === u.size);
    if (candidates.length === 1) await adopt(u.name, candidates[0].name);
  }
  const skip = new Set(skipped);

  const totalBytes = info.files.reduce((s, f) => s + f.length, 0);
  let doneBytes = 0;
  const missing: number[] = [];
  for (let i = 0; i < info.files.length; i++) {
    const f = info.files[i];
    progress('verifying', doneBytes, totalBytes, f.name);
    if (!present.has(f.name)) {
      // Missing entirely: every slice is needed, unless the user unticked it.
      if (!skip.has(f.name)) {
        for (let k = 0; k < f.sliceCount; k++) missing.push(f.firstSlice + k);
        base.damagedFiles.push(f.name);
      }
      doneBytes += f.length;
      continue;
    }
    const file = await getFile(dir, f.name);
    const start = doneBytes;
    let r = await verifyFile(engine, set, i, file, false, (n) => {
      doneBytes += n;
      progress('verifying', doneBytes, totalBytes, f.name);
    });
    if (!r.ok && f.hasChecks && r.damaged.length === f.sliceCount && file.size === f.length) {
      // Every CRC matched but the MD5 didn't: check each slice by MD5 too.
      r = await verifyFile(engine, set, i, file, true, () => {});
    }
    doneBytes = start + f.length;
    if (!r.ok) {
      if (r.damaged.length === 0 && file.size > f.length) {
        // Only trailing junk: trim it.
        const w = await openWriter(dir, f.name, target.kind);
        await w.truncate(f.length);
        await w.close();
        continue;
      }
      base.damagedFiles.push(f.name);
      for (const k of r.damaged) missing.push(f.firstSlice + k);
    }
  }
  progress('verifying', totalBytes, totalBytes, '');
  state = { set, info, missing };
  base.need = missing.length;
  base.ok = missing.length === 0;
  return base;
}

type Recovery = { file: File; exponent: number; dataOffset: number };

/** Finds valid recovery packets of this set in the volume files. */
async function scanRecovery(engine: Engine, set: Par2Set, info: Par2Info, files: File[], need: number): Promise<Recovery[]> {
  const out: Recovery[] = [];
  const seen = new Set<number>();
  for (const file of files) {
    let off = 0;
    while (off + 64 <= file.size && out.length < need) {
      const head = engine.par2_header(await readRange(file, off, off + 64));
      if (!head) {
        // Damaged: search forward for the next packet magic.
        const next = await findMagic(file, off + 1);
        if (next < 0) break;
        off = next;
        continue;
      }
      const h = JSON.parse(head) as { len: number; type: string; setId: string };
      if (h.type === 'recovery' && h.setId === info.setId && h.len <= info.sliceSize + 68) {
        const packet = await readRange(file, off, off + h.len);
        const r = set.recovery_packet(packet);
        if (r) {
          const rec = JSON.parse(r) as { exponent: number; dataOffset: number; dataLen: number };
          if (!seen.has(rec.exponent)) {
            seen.add(rec.exponent);
            out.push({ file, exponent: rec.exponent, dataOffset: off + rec.dataOffset });
          }
        }
      }
      off += Math.max(64, h.len);
    }
  }
  return out;
}

const MAGIC = new TextEncoder().encode('PAR2\0PKT');

async function findMagic(file: File, from: number): Promise<number> {
  for (let off = from; off < file.size; off += CHUNK - 8) {
    const buf = await readRange(file, off, Math.min(file.size, off + CHUNK));
    outer: for (let i = 0; i + 8 <= buf.length; i++) {
      for (let j = 0; j < 8; j++) if (buf[i + j] !== MAGIC[j]) continue outer;
      return off + i;
    }
  }
  return -1;
}

async function repair(target: Target, folder: string, volumes: string[]): Promise<FromPost> {
  if (!state) return { type: 'repaired', ok: false, message: 'Nothing to repair.', repairedSlices: 0 };
  const engine = await loadEngine(testCa);
  const dir = await openJobDir(target, folder);
  const { set, info, missing } = state;
  const m = missing.length;
  const volFiles: File[] = [];
  for (const v of volumes) if (await exists(dir, v)) volFiles.push(await getFile(dir, v));
  const rec = await scanRecovery(engine, set, info, volFiles, m);
  if (rec.length < m) {
    return {
      type: 'repaired',
      ok: false,
      message: `Repair failed: ${m} recovery blocks are needed but only ${rec.length} could be read.`,
      repairedSlices: 0,
    };
  }
  const S = info.sliceSize;
  const stripe = Math.max(4, Math.min(S, Math.floor(REPAIR_BUDGET / m / 4) * 4));
  const exps = rec.map((r) => r.exponent);
  const missingSet = new Set(missing);
  const where = (g: number) => {
    const f = info.files.find((x) => g >= x.firstSlice && g < x.firstSlice + x.sliceCount)!;
    return { f, local: g - f.firstSlice };
  };
  const passes = Math.ceil(S / stripe);
  const total = passes * (info.totalSlices + m);
  let done = 0;
  for (let off = 0; off < S; off += stripe) {
    const len = Math.min(stripe, S - off);
    const r = new engine.Par2Repairer(info.totalSlices, Uint32Array.from(missing), Uint32Array.from(exps), len + (len % 2));
    for (const x of rec) {
      r.add_recovery(x.exponent, await readRange(x.file, x.dataOffset + off, x.dataOffset + off + len));
      progress('repairing', ++done, total, '');
    }
    for (const f of info.files) {
      const present = await exists(dir, f.name);
      const file = present ? await getFile(dir, f.name) : null;
      for (let k = 0; k < f.sliceCount; k++) {
        const g = f.firstSlice + k;
        if (missingSet.has(g)) continue;
        const start = k * S + off;
        const end = Math.min(f.length, start + len);
        const data = file && start < end ? await readRange(file, start, Math.min(end, file.size)) : new Uint8Array(0);
        r.add_input(g, data);
        progress('repairing', ++done, total, f.name);
      }
    }
    let out: Uint8Array;
    try {
      out = r.solve();
    } catch (e) {
      return { type: 'repaired', ok: false, message: `Repair failed: ${(e as Error).message}`, repairedSlices: 0 };
    }
    const stride = len + (len % 2);
    // Write this stripe of every rebuilt slice, one file at a time.
    const byFile = new Map<string, { local: number; idx: number }[]>();
    missing.forEach((g, idx) => {
      const { f, local } = where(g);
      const list = byFile.get(f.name) ?? [];
      list.push({ local, idx });
      byFile.set(f.name, list);
    });
    for (const [name, items] of byFile) {
      const f = info.files.find((x) => x.name === name)!;
      const w = await openWriter(dir, name, target.kind);
      for (const { local, idx } of items) {
        const start = local * S + off;
        const end = Math.min(f.length, start + len);
        if (start < end) await w.write(start, out.subarray(idx * stride, idx * stride + (end - start)));
      }
      await w.truncate(f.length);
      await w.close();
    }
  }

  // Check the result.
  const damaged = new Set(missing.map((g) => where(g).f.name));
  for (const name of damaged) {
    const i = info.files.findIndex((x) => x.name === name);
    const r = await verifyFile(engine, set, i, await getFile(dir, name), false, () => {});
    if (!r.ok) {
      return { type: 'repaired', ok: false, message: `Repair failed: ${name} still doesn't match its checksum.`, repairedSlices: 0 };
    }
  }
  return { type: 'repaired', ok: true, message: `Repaired ${m} ${m === 1 ? 'block' : 'blocks'}.`, repairedSlices: m };
}

async function extract(
  target: Target,
  folder: string,
  files: string[],
  password: string | null,
  cleanup: boolean,
  par2Files: string[],
): Promise<FromPost> {
  const dir = await openJobDir(target, folder);
  const present = new Set(await list(dir));
  const names = files.filter((f) => present.has(f));
  const { extractAll } = await import('../archive/extract');
  const r = await extractAll(dir, target.kind, names, password, (done, total, detail) =>
    progress('extracting', done, total, detail),
  );
  const removed: string[] = [];
  if (cleanup && r.ok && r.extractedSets > 0) {
    for (const n of [...r.consumed, ...par2Files]) {
      await remove(dir, n);
      removed.push(n);
    }
  }
  const final = (await list(dir)).filter((n) => !n.endsWith('.crswap'));
  return {
    type: 'extracted',
    ok: r.ok,
    files: final,
    messages: r.messages,
    encrypted: r.encrypted,
    removed,
  };
}

async function handle(msg: ToPost) {
  try {
    switch (msg.type) {
      case 'verify':
        send(await verify(msg.target, msg.folder, msg.par2, msg.files, msg.skipped));
        break;
      case 'repair':
        send(await repair(msg.target, msg.folder, msg.volumes));
        break;
      case 'extract':
        send(await extract(msg.target, msg.folder, msg.files, msg.password, msg.cleanup, msg.par2Files));
        break;
    }
  } catch (e) {
    const message = (e as Error)?.message ?? String(e);
    if (msg.type === 'verify') {
      send({ type: 'verified', ok: false, need: 0, sliceSize: 0, renames: [], names: [], damagedFiles: [], error: message });
    } else if (msg.type === 'repair') {
      send({ type: 'repaired', ok: false, message: `Repair failed: ${message}`, repairedSlices: 0 });
    } else {
      send({ type: 'extracted', ok: false, files: [], messages: [`Extraction failed: ${message}`], encrypted: false, removed: [] });
    }
  }
}

self.onmessage = (e: MessageEvent<{ type: 'wire'; testCa: Uint8Array | null }>) => {
  if (e.data?.type !== 'wire') return;
  coord = e.ports[0];
  testCa = e.data.testCa;
  coord.onmessage = (m: MessageEvent<ToPost>) => void handle(m.data);
};
