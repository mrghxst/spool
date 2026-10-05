<script lang="ts">
  import { onMount } from 'svelte';
  import type { GridModel } from '../state.svelte';
  import { Cell } from '../types';

  let { grid, label }: { grid: GridModel; label: string } = $props();

  let wrap: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let height = $state(0);

  const SIZES = [14, 12, 10, 8, 6, 5, 4, 3, 2];
  const MAX_HEIGHT = 280;

  type Layout = { size: number; gap: number; cols: number; rows: number };

  function layout(n: number, width: number): Layout {
    let pick: Layout | null = null;
    for (const size of SIZES) {
      const gap = size >= 6 ? 2 : 1;
      const cols = Math.max(1, Math.floor((width + gap) / (size + gap)));
      const rows = Math.ceil(n / cols);
      pick = { size, gap, cols, rows };
      if (rows * (size + gap) - gap <= MAX_HEIGHT) break;
    }
    return pick!;
  }

  function color(name: string): string {
    return getComputedStyle(canvas).getPropertyValue(name).trim() || '#000';
  }

  function hatch(ctx: CanvasRenderingContext2D, fg: string, bg: string, dpr: number): CanvasPattern | null {
    const s = Math.max(4, Math.round(4 * dpr));
    const c = document.createElement('canvas');
    c.width = c.height = s;
    const p = c.getContext('2d')!;
    p.fillStyle = fg;
    p.fillRect(0, 0, s, s);
    p.strokeStyle = bg;
    p.lineWidth = Math.max(1, dpr);
    p.beginPath();
    p.moveTo(-1, s + 1);
    p.lineTo(s + 1, -1);
    p.moveTo(s - 1, s + 1);
    p.lineTo(s + 1, s - 1);
    p.moveTo(-1, 1);
    p.lineTo(1, -1);
    p.stroke();
    return ctx.createPattern(c, 'repeat');
  }

  let lastVersion = -1;
  let lastWidth = 0;
  let lastTheme = '';

  function draw(force = false) {
    if (!canvas) return;
    const width = wrap.clientWidth;
    const theme = `${document.documentElement.dataset.theme ?? ''}${matchMedia('(prefers-color-scheme: dark)').matches}`;
    if (!force && grid.version === lastVersion && width === lastWidth && theme === lastTheme) return;
    lastVersion = grid.version;
    lastWidth = width;
    lastTheme = theme;

    const order = grid.order;
    const states = grid.states;
    const n = order.length;
    const L = layout(n, width);
    const h = n === 0 ? 0 : L.rows * (L.size + L.gap) - L.gap;
    height = h;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(h * dpr);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${h}px`;
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, h);

    const fg = color('--fg');
    const bg = color('--bg');
    const border = color('--border');
    const strong = color('--border-strong');
    const danger = color('--danger');
    const pattern = hatch(ctx, fg, bg, dpr);
    const s = L.size;
    const outline = s >= 4;
    const step = s + L.gap;
    const inset = 0.5;

    for (let i = 0; i < n; i++) {
      const x = (i % L.cols) * step;
      const y = Math.floor(i / L.cols) * step;
      switch (states[order[i]]) {
        case Cell.done:
          ctx.fillStyle = fg;
          ctx.fillRect(x, y, s, s);
          break;
        case Cell.backup:
          ctx.globalAlpha = 0.55;
          ctx.fillStyle = fg;
          ctx.fillRect(x, y, s, s);
          ctx.globalAlpha = 1;
          break;
        case Cell.repaired:
          ctx.fillStyle = pattern ?? fg;
          ctx.fillRect(x, y, s, s);
          break;
        case Cell.missing:
          if (outline) {
            ctx.strokeStyle = danger;
            ctx.lineWidth = 1;
            ctx.strokeRect(x + inset, y + inset, s - 1, s - 1);
          } else {
            ctx.fillStyle = danger;
            ctx.fillRect(x, y, s, s);
          }
          break;
        case Cell.active:
          if (outline) {
            ctx.strokeStyle = strong;
            ctx.lineWidth = 1;
            ctx.strokeRect(x + inset, y + inset, s - 1, s - 1);
          } else {
            ctx.fillStyle = strong;
            ctx.fillRect(x, y, s, s);
          }
          break;
        default:
          if (outline) {
            ctx.strokeStyle = border;
            ctx.lineWidth = 1;
            ctx.strokeRect(x + inset, y + inset, s - 1, s - 1);
          } else {
            ctx.fillStyle = border;
            ctx.fillRect(x, y, s, s);
          }
      }
    }
  }

  onMount(() => {
    let raf = 0;
    const loop = () => {
      draw();
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  });
</script>

<div class="grid" bind:this={wrap} style:min-height={`${height}px`} role="img" aria-label={label}>
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
</div>

<style>
  .grid {
    width: 100%;
    line-height: 0;
  }
  canvas {
    display: block;
    max-width: 100%;
  }
</style>
