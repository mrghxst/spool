import { describe, expect, it } from 'vitest';
import { archiveSets, downloadOrder, jobFolderName, nextProvider, pickVolumes, uniqueName, volumeBlocks } from './plan';

describe('downloadOrder', () => {
  it('fetches the smallest PAR2 index first, then selected files, never recovery volumes', () => {
    const order = downloadOrder([
      { idx: 0, name: 'b.part02.rar', kind: 'archive', bytes: 100, selected: true },
      { idx: 1, name: 'b.vol00+01.par2', kind: 'par2vol', bytes: 10, selected: true },
      { idx: 2, name: 'b.par2', kind: 'par2', bytes: 5, selected: true },
      { idx: 3, name: 'b.part10.rar', kind: 'archive', bytes: 100, selected: true },
      { idx: 4, name: 'b.part01.rar', kind: 'archive', bytes: 100, selected: true },
      { idx: 5, name: 'b.nfo', kind: 'other', bytes: 1, selected: false },
      { idx: 6, name: 'other.par2', kind: 'par2', bytes: 50, selected: true },
    ]);
    expect(order).toEqual([2, 4, 0, 3]);
  });
});

describe('nextProvider', () => {
  const p = (usable = true) => ({ usable });
  it('walks the list in order, skipping tried and unusable providers', () => {
    const list = [p(), p(false), p(), p()];
    expect(nextProvider(0, list)).toBe(0);
    expect(nextProvider(0b0001, list)).toBe(2);
    expect(nextProvider(0b0101, list)).toBe(3);
    expect(nextProvider(0b1101, list)).toBe(-1);
  });
});

describe('pickVolumes', () => {
  const v = (idx: number, blocks: number) => ({ idx, blocks, bytes: blocks * 1000 });
  it('covers the need without overshooting much', () => {
    const vols = [v(0, 1), v(1, 2), v(2, 4), v(3, 8), v(4, 16)];
    const pick = pickVolumes(vols, 5)!;
    const total = pick.reduce((s, x) => s + x.blocks, 0);
    expect(total).toBeGreaterThanOrEqual(5);
    expect(total).toBeLessThanOrEqual(8);
    expect(pickVolumes(vols, 0)).toEqual([]);
    expect(pickVolumes(vols, 100)).toBeNull();
    expect(pickVolumes([v(0, 2), v(1, 2)], 3)!.length).toBe(2);
  });
  it('prefers one small volume over a big one', () => {
    const pick = pickVolumes([v(0, 1), v(1, 32)], 1)!;
    expect(pick.map((x) => x.idx)).toEqual([0]);
  });
});

describe('volumeBlocks', () => {
  it('reads the block count from the name', () => {
    expect(volumeBlocks('x.vol003+04.par2', 0, 0)).toBe(4);
    expect(volumeBlocks('x.vol3-4.par2', 0, 0)).toBe(4);
    expect(volumeBlocks('a1b2c3', 10_000, 1000)).toBe(9);
  });
});

describe('archiveSets', () => {
  it('groups RAR parts, old-style volumes and split 7z', () => {
    const sets = archiveSets([
      'movie.part02.rar',
      'movie.part01.rar',
      'movie.part10.rar',
      'old.rar',
      'old.r01',
      'old.r00',
      'data.7z.002',
      'data.7z.001',
      'single.7z',
      'docs.zip',
      'readme.txt',
    ]);
    expect(sets).toEqual([
      { first: 'movie.part01.rar', parts: ['movie.part01.rar', 'movie.part02.rar', 'movie.part10.rar'], format: 'rar', split: false },
      { first: 'old.rar', parts: ['old.rar', 'old.r00', 'old.r01'], format: 'rar', split: false },
      { first: 'data.7z.001', parts: ['data.7z.001', 'data.7z.002'], format: '7z', split: true },
      { first: 'single.7z', parts: ['single.7z'], format: '7z', split: false },
      { first: 'docs.zip', parts: ['docs.zip'], format: 'zip', split: false },
    ]);
  });
});

describe('names', () => {
  it('makes names unique', () => {
    const taken = new Set(['a.iso', 'a (2).iso']);
    expect(uniqueName('a.iso', taken)).toBe('a (3).iso');
    expect(uniqueName('b.iso', taken)).toBe('b.iso');
    expect(uniqueName('noext', new Set(['noext']))).toBe('noext (2)');
  });
  it('makes folder-safe job names', () => {
    expect(jobFolderName('My: Job?.nzb')).toBe('My_ Job_');
    expect(jobFolderName('...')).toBe('download');
  });
});
