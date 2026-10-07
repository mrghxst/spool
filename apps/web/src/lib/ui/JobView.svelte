<script lang="ts">
  import { formatBytes, formatDuration, formatSpeed, plural, SpeedMeter } from '../core/format';
  import { app, canPickFolder, type JobEntry } from '../state.svelte';
  import type { Phase } from '../types';
  import SegmentGrid from './SegmentGrid.svelte';

  let { job }: { job: JobEntry } = $props();

  const meter = new SpeedMeter();
  let speed = $state(0);
  let shownFiles = $state<{ name: string; size: number }[] | null>(null);
  let saving = $state(false);

  const p = $derived(job.progress);
  const phase = $derived<Phase>(
    job.status === 'done' ? 'done' : job.status === 'failed' ? 'failed' : job.status === 'cancelled' ? 'cancelled' : (p?.phase ?? 'queued'),
  );

  $effect(() => {
    if (!p) return;
    if (p.phase === 'downloading' || p.phase === 'connecting' || p.phase === 'repairing') {
      speed = meter.update(p.received, performance.now());
    } else {
      speed = 0;
    }
  });

  const eta = $derived(p && speed > 0 && p.total > p.received ? (p.total - p.received) / speed : NaN);
  const elapsed = $derived(job.finishedAt && job.startedAt ? (job.finishedAt - job.startedAt) / 1000 : NaN);
  const finished = $derived(Number.isFinite(elapsed));

  const steps: { phase: Phase; label: string }[] = [
    { phase: 'downloading', label: 'Downloading' },
    { phase: 'verifying', label: 'Verifying' },
    { phase: 'repairing', label: 'Repairing' },
    { phase: 'extracting', label: 'Extracting' },
    { phase: 'done', label: 'Done' },
  ];
  const order: Phase[] = ['queued', 'connecting', 'downloading', 'verifying', 'repairing', 'extracting', 'done'];

  function stepState(s: Phase): 'done' | 'current' | 'todo' {
    const cur = phase === 'connecting' ? 'downloading' : phase;
    const a = order.indexOf(s);
    const b = order.indexOf(cur);
    if (cur === 'done') return 'done';
    if (a < b) return 'done';
    if (a === b) return 'current';
    return 'todo';
  }

  // A job that failed before receiving anything: the error says it all.
  const nothingYet = $derived(job.status === 'failed' && (p?.received ?? 0) === 0);
  const running = $derived(job.status === 'running' || job.status === 'queued');
  const percent = $derived(p && p.total > 0 ? Math.min(100, (p.received / p.total) * 100) : 0);
  const postPercent = $derived(p && p.postTotal > 0 ? Math.min(100, (p.postDone / p.postTotal) * 100) : 0);
  const gridLabel = $derived(
    p
      ? `${p.segmentsDone} of ${p.segmentsTotal} articles downloaded, ${p.segmentsBackup} from backup, ${p.segmentsMissing} missing`
      : 'Waiting to start',
  );

  async function showFiles() {
    shownFiles = shownFiles ? null : await app.listFiles(job.id);
  }

  async function save() {
    saving = true;
    try {
      await app.saveFiles(job.id);
    } finally {
      saving = false;
    }
  }
</script>

<section class="card job" aria-labelledby={`job-${job.id}`}>
  <div class="head">
    <div class="title">
      <h1 id={`job-${job.id}`}>{job.summary.name}</h1>
      {#if job.status === 'queued'}
        <p class="muted">Queued. Starts when the current download finishes.</p>
      {/if}
    </div>
    {#if running}
      <button class="btn" onclick={() => app.cancel(job.id)}>Cancel</button>
    {:else}
      <button class="btn ghost" onclick={() => app.dismiss(job.id)}>Dismiss</button>
    {/if}
  </div>

  <ol class="steps" aria-label="Progress">
    {#each steps as s (s.phase)}
      <li data-state={stepState(s.phase)} aria-current={stepState(s.phase) === 'current' ? 'step' : undefined}>
        {s.label}
      </li>
    {/each}
  </ol>

  {#if p && !nothingYet}
    <dl class="stats">
      {#if finished}
        <div>
          <dt>Average speed</dt>
          <dd class="mono" data-testid="average-speed">
            {job.result && job.result.transferMs > 0
              ? formatSpeed((job.result.bytes * 1000) / job.result.transferMs)
              : elapsed > 0
                ? formatSpeed(p.received / elapsed)
                : '–'}
          </dd>
        </div>
        <div>
          <dt>Took</dt>
          <dd class="mono">{formatDuration(Math.max(1, elapsed))}</dd>
        </div>
      {:else}
        <div>
          <dt>Speed</dt>
          <dd class="mono">{speed > 0 ? formatSpeed(speed) : '–'}</dd>
        </div>
        <div>
          <dt>Time left</dt>
          <dd class="mono">{Number.isFinite(eta) ? formatDuration(Math.max(1, eta)) : '–'}</dd>
        </div>
      {/if}
      <div>
        <dt>Received</dt>
        <dd class="mono">{formatBytes(p.received)} of {formatBytes(p.total)}</dd>
      </div>
      <div>
        <dt>Articles</dt>
        <dd class="mono">{p.segmentsDone.toLocaleString('en-US')} of {p.segmentsTotal.toLocaleString('en-US')}</dd>
      </div>
    </dl>
    <div class="bar" role="progressbar" aria-label="Download progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(percent)}>
      <span style:width={`${percent}%`}></span>
    </div>
  {/if}

  {#if !nothingYet}<SegmentGrid grid={job.grid} label={gridLabel} />{/if}

  {#if p && !nothingYet}
    <ul class="legend small muted" aria-label="Legend">
      {#if running}<li><span class="sw pending"></span>Waiting</li>{/if}
      <li><span class="sw done"></span>Downloaded <span class="mono">{(p.segmentsDone - p.segmentsBackup).toLocaleString('en-US')}</span></li>
      {#if p.segmentsBackup > 0}
        <li data-testid="backup-count"><span class="sw backup"></span>From backup <span class="mono">{p.segmentsBackup.toLocaleString('en-US')}</span></li>
      {/if}
      {#if p.segmentsMissing > 0}
        <li><span class="sw missing"></span>Missing <span class="mono">{p.segmentsMissing.toLocaleString('en-US')}</span></li>
      {/if}
      {#if p.segmentsRepaired > 0}
        <li data-testid="repaired-count"><span class="sw repaired"></span>Repaired <span class="mono">{p.segmentsRepaired.toLocaleString('en-US')}</span></li>
      {/if}
    </ul>
  {/if}

  {#if p && (phase === 'verifying' || phase === 'repairing' || phase === 'extracting') && p.postTotal > 0}
    <div class="post small">
      <span>{phase === 'verifying' ? 'Verifying' : phase === 'repairing' ? 'Repairing' : 'Extracting'}{p.detail ? ` ${p.detail}` : ''}</span>
      <span class="mono muted">{Math.round(postPercent)}%</span>
    </div>
  {:else if p?.detail && running}
    <p class="small muted">{p.detail}</p>
  {/if}

  {#if p && job.status !== 'failed'}
    {#each p.providers.filter((x) => x.error) as prov (prov.id)}
      <p class="small danger" role="alert">{prov.name}: {prov.error}</p>
    {/each}
  {/if}

  {#if p && running}
    <details class="files">
      <summary class="small">Files</summary>
      <ul>
        {#each p.files as f (f.idx)}
          <li class="small">
            <span class="mono name">{f.name}</span>
            <span class="mono muted">
              {#if f.state === 'done'}Done{:else if f.state === 'incomplete'}<span class="danger">{plural(f.missing, 'article')} missing</span>{:else if f.state === 'waiting'}Waiting{:else}{Math.round(
                  (f.done / Math.max(1, f.bytes)) * 100,
                )}%{/if}
            </span>
          </li>
        {/each}
      </ul>
    </details>
  {/if}

  {#if job.status === 'failed'}
    <p class="danger" role="alert">{job.error}</p>
  {/if}

  {#if job.status === 'done' && job.result}
    <div class="result">
      <p>
        {#if job.result.ok}
          Done. {plural(job.result.files.length, 'file')} ready{job.result.repaired ? `, ${plural(job.result.repaired, 'article')} repaired with PAR2` : ''}.
        {:else}
          Finished with problems.
        {/if}
      </p>
      {#each job.result.messages as m, i (i)}
        <p class="small" class:danger={!job.result.ok}>{m}</p>
      {/each}
      {#if job.result.password}
        <p class="small">Archive password from the NZB: <span class="mono">{job.result.password}</span></p>
      {/if}
      <div class="inline">
        {#if job.target?.kind === 'fsa'}
          <button class="btn primary" onclick={showFiles}>{shownFiles ? 'Hide files' : 'Show files'}</button>
          <span class="small muted">Saved in <span class="mono">{app.folderName}/{job.folder}</span></span>
        {:else}
          <button class="btn primary" onclick={save} disabled={saving}>{job.saved ? 'Save files again' : 'Save files'}</button>
          {#if job.saved}<span class="small muted">Saved to your downloads.</span>{/if}
        {/if}
      </div>
      {#if shownFiles}
        <ul class="final">
          {#each shownFiles as f (f.name)}
            <li class="small"><span class="mono name">{f.name}</span><span class="mono muted">{formatBytes(f.size)}</span></li>
          {/each}
        </ul>
      {/if}
      {#if !canPickFolder && job.target?.kind === 'opfs' && !job.saved}
        <p class="small muted">Your browser keeps finished files in temporary storage until you save them.</p>
      {/if}
    </div>
  {/if}
</section>

<style>
  .job {
    padding: 24px;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 20px;
    min-width: 0;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
  }
  .title {
    display: grid;
    gap: 4px;
    min-width: 0;
  }
  h1 {
    overflow-wrap: anywhere;
  }
  .steps {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 20px;
    list-style: none;
    margin: 0;
    padding: 0;
    font-size: var(--t13);
  }
  .steps li {
    color: var(--muted);
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .steps li::before {
    content: '';
    width: 6px;
    height: 6px;
    border-radius: 50%;
    border: 1px solid var(--border-strong);
  }
  .steps li[data-state='done'] {
    color: var(--fg);
  }
  .steps li[data-state='done']::before {
    background: var(--fg);
    border-color: var(--fg);
  }
  .steps li[data-state='current'] {
    color: var(--fg);
    font-weight: 600;
  }
  .steps li[data-state='current']::before {
    border-color: var(--fg);
    box-shadow: inset 0 0 0 1.5px var(--bg);
    background: var(--fg);
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 16px;
    margin: 0;
  }
  .stats div {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  dt {
    font-size: var(--t13);
    color: var(--muted);
  }
  dd {
    margin: 0;
    font-size: var(--t16);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .bar {
    height: 2px;
    background: var(--border);
    margin-top: -8px;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--fg);
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    list-style: none;
    margin: -8px 0 0;
    padding: 0;
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .sw {
    width: 10px;
    height: 10px;
    display: inline-block;
  }
  .sw.pending {
    border: 1px solid var(--border-strong);
  }
  .sw.done {
    background: var(--fg);
  }
  .sw.backup {
    background: var(--fg);
    opacity: 0.55;
  }
  .sw.missing {
    border: 1px solid var(--danger);
  }
  .sw.repaired {
    background: repeating-linear-gradient(135deg, var(--fg) 0 2px, var(--bg) 2px 3px);
  }
  .post {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }
  .files summary {
    cursor: pointer;
    color: var(--muted);
    width: max-content;
  }
  .files ul,
  .final {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    max-height: 280px;
    overflow-y: auto;
  }
  .files li,
  .final li {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 12px;
    border-top: 1px solid var(--border);
  }
  .files li:first-child,
  .final li:first-child {
    border-top: 0;
  }
  .name {
    overflow-wrap: anywhere;
    min-width: 0;
  }
  .result {
    display: grid;
    gap: 10px;
    padding-top: 16px;
    border-top: 1px solid var(--border);
  }
  .inline {
    display: flex;
    gap: 12px;
    align-items: center;
    flex-wrap: wrap;
  }
  @media (max-width: 640px) {
    .job {
      padding: 16px;
    }
    dd {
      white-space: normal;
      font-size: var(--t14);
    }
    .stats {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
