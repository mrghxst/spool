<script lang="ts">
  import { formatBytes, plural } from '../core/format';
  import { app, canPickFolder } from '../state.svelte';
  import type { JobSummary } from '../types';

  let { summary }: { summary: JobSummary } = $props();

  const kindLabel: Record<string, string> = {
    par2: 'PAR2 index',
    par2vol: 'Recovery',
    archive: 'Archive',
    other: '',
  };

  const choosable = $derived(summary.files.filter((f) => f.kind !== 'par2vol'));
  const recovery = $derived(summary.files.filter((f) => f.kind === 'par2vol'));
  const selected = $derived(new Set(app.pendingSelected));
  const selectedBytes = $derived(
    summary.files.filter((f) => selected.has(f.idx) && f.kind !== 'par2vol').reduce((s, f) => s + f.bytes, 0),
  );
  const selectedCount = $derived(choosable.filter((f) => selected.has(f.idx)).length);
  const allOn = $derived(choosable.every((f) => selected.has(f.idx)));

  function toggle(idx: number, on: boolean) {
    const s = new Set(app.pendingSelected);
    if (on) s.add(idx);
    else s.delete(idx);
    app.pendingSelected = [...s];
  }

  function toggleAll(on: boolean) {
    app.pendingSelected = on ? choosable.map((f) => f.idx) : [];
  }
</script>

<section class="card summary" aria-labelledby="summary-title">
  <div class="head">
    <h1 id="summary-title">{summary.name}</h1>
    <p class="muted">
      <span class="mono">{formatBytes(selectedBytes)}</span> in {plural(selectedCount, 'file')}{#if recovery.length},
        plus {plural(recovery.length, 'recovery file')} if repair needs them{/if}
    </p>
    {#if summary.password}
      <p class="small muted">This NZB includes an archive password.</p>
    {/if}
  </div>

  <div class="files" role="group" aria-label="Files to download">
    <label class="check all">
      <input type="checkbox" checked={allOn} onchange={(e) => toggleAll((e.target as HTMLInputElement).checked)} />
      <span>All files</span>
    </label>
    <ul>
      {#each choosable as f (f.idx)}
        <li>
          <label class="check">
            <input
              type="checkbox"
              checked={selected.has(f.idx)}
              onchange={(e) => toggle(f.idx, (e.target as HTMLInputElement).checked)}
            />
            <span class="name mono">{f.name}</span>
          </label>
          <span class="kind small muted">{kindLabel[f.kind]}</span>
          <span class="size small muted mono">{formatBytes(f.bytes)}</span>
        </li>
      {/each}
    </ul>
  </div>

  <div class="actions">
    <button class="btn" onclick={() => app.discardPending()}>Discard</button>
    <button class="btn primary" disabled={app.pendingSelected.length === 0 || !app.hasProviders} onclick={() => app.download()}>
      Download
    </button>
  </div>
  {#if !app.hasProviders}
    <p class="small muted note">Add a Usenet provider before you download.</p>
  {:else if canPickFolder && !app.folderName}
    <p class="small muted note">You'll choose a download folder next.</p>
  {/if}
</section>

<style>
  .summary {
    padding: 24px;
    display: grid;
    gap: 20px;
  }
  .head {
    display: grid;
    gap: 6px;
  }
  h1 {
    word-break: break-word;
  }
  .files {
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    overflow: hidden;
  }
  .all {
    padding: 10px 14px;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    font-weight: 500;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 360px;
    overflow-y: auto;
  }
  li {
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: 12px;
    align-items: center;
    padding: 8px 14px;
    border-top: 1px solid var(--border);
  }
  li:first-child {
    border-top: 0;
  }
  .name {
    overflow-wrap: anywhere;
  }
  .size {
    min-width: 64px;
    text-align: right;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .note {
    text-align: right;
    margin-top: -8px;
  }
  @media (max-width: 520px) {
    .summary {
      padding: 16px;
    }
    li {
      grid-template-columns: 1fr auto;
    }
    .kind {
      display: none;
    }
  }
</style>
