<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from './lib/state.svelte';
  import EmptyState from './lib/ui/EmptyState.svelte';
  import Footer from './lib/ui/Footer.svelte';
  import JobSummary from './lib/ui/JobSummary.svelte';
  import JobView from './lib/ui/JobView.svelte';
  import ProvidersSheet from './lib/ui/ProvidersSheet.svelte';
  import SettingsSheet from './lib/ui/SettingsSheet.svelte';
  import TopBar from './lib/ui/TopBar.svelte';

  let fileInput: HTMLInputElement;
  let dragDepth = $state(0);

  onMount(() => {
    void app.load();
  });

  function hasFiles(e: DragEvent) {
    return [...(e.dataTransfer?.types ?? [])].includes('Files');
  }

  function dragenter(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragDepth++;
  }

  function dragover(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
  }

  function dragleave(e: DragEvent) {
    if (!hasFiles(e)) return;
    dragDepth = Math.max(0, dragDepth - 1);
  }

  function drop(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragDepth = 0;
    const file = e.dataTransfer?.files[0];
    if (file) void app.openNzb(file);
  }

  function chose(e: Event) {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    if (file) void app.openNzb(file);
    input.value = '';
  }

  function beforeunload(e: BeforeUnloadEvent) {
    if (app.activeJob) {
      e.preventDefault();
      e.returnValue = '';
    }
  }
</script>

<svelte:window ondragenter={dragenter} ondragover={dragover} ondragleave={dragleave} ondrop={drop} onbeforeunload={beforeunload} />

<TopBar />

<main>
  {#if app.pending}
    <JobSummary summary={app.pending} />
  {/if}
  {#each app.jobs as job (job.id)}
    <JobView {job} />
  {/each}
  {#if !app.pending && app.jobs.length === 0}
    <EmptyState onchoose={() => fileInput.click()} />
  {:else if !app.pending && !app.activeJob}
    <div class="again">
      <button class="btn" onclick={() => fileInput.click()} disabled={app.busy}>Choose another .nzb file</button>
      {#if app.notice}<p class="small danger" role="alert">{app.notice}</p>{/if}
    </div>
  {/if}
  <input bind:this={fileInput} type="file" accept=".nzb,application/x-nzb" hidden onchange={chose} />
</main>

<Footer />

{#if dragDepth > 0}
  <div class="dropping" aria-hidden="true"><p>Drop an .nzb file</p></div>
{/if}

{#if app.sheet === 'providers'}
  <ProvidersSheet />
{:else if app.sheet === 'settings'}
  <SettingsSheet />
{/if}

<style>
  main {
    max-width: 880px;
    margin: 0 auto;
    padding: 24px 16px 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 16px;
  }
  .again {
    display: grid;
    gap: 8px;
    justify-items: start;
  }
  .dropping {
    position: fixed;
    inset: 8px;
    border: 1px dashed var(--fg);
    border-radius: var(--r-sheet);
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    display: grid;
    place-items: center;
    z-index: 60;
    pointer-events: none;
  }
  .dropping p {
    font-size: var(--t24);
    font-weight: 600;
  }
</style>
