import { describe, expect, it } from 'vitest';
import { looksObfuscated } from './names';

describe('looksObfuscated', () => {
  it('keeps readable release names', () => {
    expect(looksObfuscated('The.Odyssey.with.Dan.Snow.2026.1080p.HDTV.H264-DARKFLiX.par2')).toBe(false);
    expect(looksObfuscated('ubuntu-24.04.iso')).toBe(false);
    expect(looksObfuscated('archive.part01.rar')).toBe(false);
    expect(looksObfuscated('archive.r00')).toBe(false);
    expect(looksObfuscated('archive.7z.001')).toBe(false);
    expect(looksObfuscated('thumbnail.jpg')).toBe(false);
  });
  it('spots random names', () => {
    expect(looksObfuscated('RF8pbaCaokZ3Khoy6QkrFUOUSsKdlKy6SKtbrOMUMJyAib9n')).toBe(true);
    expect(looksObfuscated('a8f3k2j1m9x7q4w6e5r0t2y8.bin')).toBe(true);
    expect(looksObfuscated('3d2b1f0e9c8a7b6c5d4e3f2a1b0c9d8e')).toBe(true);
    expect(looksObfuscated('file-12')).toBe(true);
  });
});
