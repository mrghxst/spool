// Download planning: file order, provider routing, recovery volume choice,
// archive sets. Pure functions so they can be unit tested.

import type { FileKind } from '../types';

export type PlanFile = { idx: number; name: string; kind: FileKind; bytes: number; selected: boolean };

/**
 * Download order: the smallest PAR2 index first (real names and hashes),
 * then the selected non-PAR2 files in name order. Recovery volumes are
 * fetched later, only if repair needs them.
 */
export function downloadOrder(files: PlanFile[]): number[] {
  const index = files
    .filter((f) => f.kind === 'par2')
    .sort((a, b) => a.bytes - b.bytes)
    .slice(0, 1);
  const rest = files
    .filter((f) => f.kind !== 'par2' && f.kind !== 'par2vol' && f.selected)
    .sort((a, b) => a.name.localeCompare(b.name, 'en', { numeric: true }));
  return [...index, ...rest].map((f) => f.idx);
}

export type RouteProvider = { usable: boolean };

/**
 * The next provider (in list order) that hasn't tried a segment yet, or -1.
 * `tried` is a bit mask over provider indexes.
 */
export function nextProvider(tried: number, providers: RouteProvider[]): number {
  for (let i = 0; i < providers.length && i < 31; i++) {
    if (!providers[i].usable) continue;
    if (tried & (1 << i)) continue;
    return i;
  }
  return -1;
}

export type Volume = { idx: number; blocks: number; bytes: number };

/**
 * Picks recovery volumes that together hold at least `need` blocks, keeping
 * the download small: largest first until covered, then drop any volume
 * that isn't needed, smallest first.
 */
export function pickVolumes(volumes: Volume[], need: number): Volume[] | null {
  if (need <= 0) return [];
  const total = volumes.reduce((s, v) => s + v.blocks, 0);
  if (total < need) return null;
  const sorted = [...volumes].sort((a, b) => b.blocks - a.blocks || a.bytes - b.bytes);
  const chosen: Volume[] = [];
  let have = 0;
  for (const v of sorted) {
    if (have >= need) break;
    chosen.push(v);
    have += v.blocks;
  }
  // Try to swap the last (largest) picks for a smaller one that still covers.
  chosen.sort((a, b) => a.blocks - b.blocks);
  for (let i = 0; i < chosen.length; ) {
    if (have - chosen[i].blocks >= need) {
      have -= chosen[i].blocks;
      chosen.splice(i, 1);
    } else {
      i++;
    }
  }
  // A single smaller unchosen volume may replace the biggest pick.
  const unchosen = volumes.filter((v) => !chosen.includes(v)).sort((a, b) => a.bytes - b.bytes);
  const biggest = chosen[chosen.length - 1];
  if (biggest) {
    for (const v of unchosen) {
      if (v.bytes < biggest.bytes && have - biggest.blocks + v.blocks >= need) {
        chosen[chosen.length - 1] = v;
        break;
      }
    }
  }
  return chosen.sort((a, b) => a.idx - b.idx);
}

/** Block count of a recovery volume from its name, else an estimate. */
export function volumeBlocks(name: string, bytes: number, sliceSize: number): number {
  const m = /\.vol\d+[+-](\d+)\.par2$/i.exec(name);
  if (m) return Number(m[1]);
  return sliceSize > 0 ? Math.max(1, Math.floor(bytes / (sliceSize + 68))) : 1;
}

export type ArchiveSet = {
  /** The file to open (first volume). */
  first: string;
  /** Every part, in volume order. */
  parts: string[];
  format: 'rar' | '7z' | 'zip';
  /** Split files that must be joined (`.7z.001`, `.zip.001`). */
  split: boolean;
};

function stripNum(n: string): number {
  return Number(n.replace(/\D/g, ''));
}

/** Groups archive files into sets and finds each set's first volume. */
export function archiveSets(names: string[]): ArchiveSet[] {
  const sets: ArchiveSet[] = [];
  const used = new Set<string>();
  const lower = new Map(names.map((n) => [n.toLowerCase(), n]));
  const byPattern = (re: RegExp) =>
    names
      .map((n) => ({ n, m: re.exec(n) }))
      .filter((x): x is { n: string; m: RegExpExecArray } => x.m !== null);

  // name.partNN.rar
  const parts = byPattern(/^(.*)\.part(\d+)\.rar$/i);
  const partGroups = new Map<string, { n: string; num: number }[]>();
  for (const { n, m } of parts) {
    const key = m[1].toLowerCase();
    const list = partGroups.get(key) ?? [];
    list.push({ n, num: Number(m[2]) });
    partGroups.set(key, list);
  }
  for (const list of partGroups.values()) {
    list.sort((a, b) => a.num - b.num);
    list.forEach((p) => used.add(p.n));
    sets.push({ first: list[0].n, parts: list.map((p) => p.n), format: 'rar', split: false });
  }

  // name.rar + name.r00, name.r01 ...
  for (const n of names) {
    if (used.has(n) || !/\.rar$/i.test(n)) continue;
    const stem = n.slice(0, -4);
    const olds = names
      .filter((x) => x.toLowerCase().startsWith(stem.toLowerCase() + '.r') && /\.r\d{2,3}$/i.test(x))
      .sort((a, b) => stripNum(a.slice(stem.length)) - stripNum(b.slice(stem.length)));
    used.add(n);
    olds.forEach((o) => used.add(o));
    sets.push({ first: n, parts: [n, ...olds], format: 'rar', split: false });
  }

  // name.7z.001 / name.zip.001 (plain split files)
  for (const { n, m } of byPattern(/^(.*\.(7z|zip))\.(\d{3})$/i)) {
    if (used.has(n) || Number(m[3]) !== 1) continue;
    const base = m[1];
    const vols = names
      .filter((x) => new RegExp(`^${escapeRe(base)}\\.\\d{3}$`, 'i').test(x))
      .sort((a, b) => stripNum(a.slice(-3)) - stripNum(b.slice(-3)));
    vols.forEach((v) => used.add(v));
    sets.push({ first: n, parts: vols, format: m[2].toLowerCase() as '7z' | 'zip', split: true });
  }

  // Plain .7z and .zip (with .z01... for spanned zip).
  for (const n of names) {
    if (used.has(n)) continue;
    if (/\.7z$/i.test(n)) {
      used.add(n);
      sets.push({ first: n, parts: [n], format: '7z', split: false });
    } else if (/\.zip$/i.test(n)) {
      const stem = n.slice(0, -4);
      const spans = names
        .filter((x) => new RegExp(`^${escapeRe(stem)}\\.z\\d{2}$`, 'i').test(x))
        .sort((a, b) => stripNum(a.slice(-2)) - stripNum(b.slice(-2)));
      used.add(n);
      spans.forEach((s) => used.add(s));
      sets.push({ first: n, parts: [...spans, n], format: 'zip', split: false });
    }
  }
  void lower;
  return sets;
}

function escapeRe(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Makes `name` unique among `taken` by adding " (2)", " (3)" before the extension. */
export function uniqueName(name: string, taken: Set<string>): string {
  if (!taken.has(name.toLowerCase())) return name;
  const dot = name.lastIndexOf('.');
  const stem = dot > 0 ? name.slice(0, dot) : name;
  const ext = dot > 0 ? name.slice(dot) : '';
  for (let i = 2; ; i++) {
    const candidate = `${stem} (${i})${ext}`;
    if (!taken.has(candidate.toLowerCase())) return candidate;
  }
}

/** A folder-safe job name. */
export function jobFolderName(name: string): string {
  const cleaned = name
    .replace(/\.nzb$/i, '')
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_')
    .replace(/^[.\s]+|[.\s]+$/g, '')
    .slice(0, 120);
  return cleaned || 'download';
}
