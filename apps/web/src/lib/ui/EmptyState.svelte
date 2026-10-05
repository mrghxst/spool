<script lang="ts">
  import { app, newProvider } from '../state.svelte';

  let { onchoose }: { onchoose: () => void } = $props();
</script>

<div class="empty">
  {#if !app.hasProviders && app.loaded}
    <div class="card prompt">
      <div>
        <h2>Add a Usenet provider to start</h2>
        <p class="small muted">Your provider's server, username and password. They stay in this browser.</p>
      </div>
      <button
        class="btn primary"
        onclick={() => {
          app.editing = newProvider();
          app.sheet = 'providers';
        }}>Add provider</button
      >
    </div>
  {/if}
  <div class="drop">
    <p class="line">Drop an .nzb file</p>
    <button class="btn" onclick={onchoose} disabled={app.busy}>{app.busy ? 'Reading file…' : 'Choose file'}</button>
    {#if app.notice}
      <p class="small danger" role="alert">{app.notice}</p>
    {/if}
  </div>
</div>

<style>
  .empty {
    min-height: calc(100dvh - 56px - 96px);
    display: flex;
    flex-direction: column;
    gap: 24px;
  }
  .prompt {
    display: flex;
    gap: 16px;
    align-items: center;
    justify-content: space-between;
    padding: 20px 24px;
  }
  .prompt > div {
    display: grid;
    gap: 4px;
  }
  .drop {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    text-align: center;
    padding: 48px 16px;
  }
  .line {
    font-size: var(--t24);
    font-weight: 600;
    letter-spacing: -0.02em;
  }
  @media (max-width: 520px) {
    .prompt {
      flex-direction: column;
      align-items: flex-start;
    }
  }
</style>
