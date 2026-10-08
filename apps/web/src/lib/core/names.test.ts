import { describe, expect, it } from 'vitest';
import { looksObfuscated, sameFolder, shortName } from './names';

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

describe('sameFolder', () => {
  const job = 'Hitmans.Wifes.Bodyguard.2021.Extended.REPACK.UHD.BluRay.1080p.DDP.5.1.DoVi.HDR10.x265-SM737';
  it('matches the same release name with other punctuation', () => {
    expect(sameFolder('Hitmans_Wifes_Bodyguard_2021_Extended_REPACK_UHD_BluRay_1080p_DDP_5_1_DoVi_HDR10_x265-SM737', job)).toBe(true);
    // The job folder name is capped at 80 characters.
    expect(sameFolder('Hitmans_Wifes_Bodyguard_2021_Extended_REPACK_UHD_BluRay_1080p_DDP_5_1_DoVi_HDR10_x265-SM737', job.slice(0, 80))).toBe(true);
  });
  it('keeps real subfolders', () => {
    expect(sameFolder('Subs', job)).toBe(false);
    expect(sameFolder('Season 1', 'Some.Show.S01.1080p')).toBe(false);
  });
});

describe('shortName', () => {
  it('keeps short names and extensions', () => {
    expect(shortName('movie.mkv')).toBe('movie.mkv');
    const s = shortName(`${'x'.repeat(100)}.mkv`);
    expect(s.length).toBeLessThanOrEqual(60);
    expect(s.endsWith('~.mkv')).toBe(true);
  });
});
