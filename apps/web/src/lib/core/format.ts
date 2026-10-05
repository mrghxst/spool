// Formatting helpers. Numbers are shown with tabular figures in the UI.

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

/** 1536 -> "1.5 KB". Decimal units, like file managers on macOS and Linux. */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return '0 B';
  let i = 0;
  let v = n;
  while (v >= 1000 && i < UNITS.length - 1) {
    v /= 1000;
    i++;
  }
  const digits = i === 0 || v >= 100 ? 0 : v >= 10 ? 1 : 2;
  return `${v.toFixed(digits)} ${UNITS[i]}`;
}

export function formatSpeed(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** Seconds -> "45 s", "3 min", "1 h 05 min". */
export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '';
  const s = Math.round(seconds);
  if (s < 60) return `${s} s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return `${h} h ${String(m % 60).padStart(2, '0')} min`;
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n.toLocaleString('en-US')} ${n === 1 ? one : many}`;
}

/** Exponential moving average speed meter fed with cumulative byte counts. */
export class SpeedMeter {
  private lastBytes = 0;
  private lastTime = 0;
  private rate = 0;

  constructor(private readonly halfLifeMs = 2000) {}

  reset(): void {
    this.lastBytes = 0;
    this.lastTime = 0;
    this.rate = 0;
  }

  /** Feed the cumulative byte count at time `now` (ms). Returns bytes/s. */
  update(totalBytes: number, now: number): number {
    if (this.lastTime === 0) {
      this.lastTime = now;
      this.lastBytes = totalBytes;
      return 0;
    }
    const dt = now - this.lastTime;
    if (dt < 100) return this.rate;
    const instant = ((totalBytes - this.lastBytes) * 1000) / dt;
    const alpha = 1 - Math.pow(0.5, dt / this.halfLifeMs);
    this.rate = this.rate === 0 ? instant : this.rate + alpha * (instant - this.rate);
    this.lastTime = now;
    this.lastBytes = totalBytes;
    return this.rate;
  }

  get value(): number {
    return this.rate;
  }
}
