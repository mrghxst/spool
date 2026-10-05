import { describe, expect, it } from 'vitest';
import { formatBytes, formatDuration, plural, SpeedMeter } from './format';

describe('format', () => {
  it('formats bytes', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(999)).toBe('999 B');
    expect(formatBytes(1536)).toBe('1.54 KB');
    expect(formatBytes(52_400_000)).toBe('52.4 MB');
    expect(formatBytes(4.7e9)).toBe('4.70 GB');
  });
  it('formats durations', () => {
    expect(formatDuration(42)).toBe('42 s');
    expect(formatDuration(150)).toBe('3 min');
    expect(formatDuration(3900)).toBe('1 h 05 min');
  });
  it('pluralises', () => {
    expect(plural(1, 'file')).toBe('1 file');
    expect(plural(1200, 'file')).toBe('1,200 files');
  });
  it('smooths speed', () => {
    const m = new SpeedMeter(1000);
    expect(m.update(0, 1000)).toBe(0);
    expect(m.update(1_000_000, 2000)).toBeCloseTo(1_000_000);
    const next = m.update(1_500_000, 3000);
    expect(next).toBeGreaterThan(500_000);
    expect(next).toBeLessThan(1_000_000);
  });
});
