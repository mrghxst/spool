<script lang="ts">
  import { app, newProvider } from '../state.svelte';
  import ProviderForm from './ProviderForm.svelte';
  import Sheet from './Sheet.svelte';

  let dragging = $state<number | null>(null);
  let over = $state<number | null>(null);

  function close() {
    app.editing = null;
    app.sheet = null;
  }

  function drop(to: number) {
    if (dragging !== null) app.moveProvider(dragging, to);
    dragging = null;
    over = null;
  }

  function keymove(e: KeyboardEvent, i: number) {
    if (e.altKey && e.key === 'ArrowUp') {
      e.preventDefault();
      app.moveProvider(i, i - 1);
    } else if (e.altKey && e.key === 'ArrowDown') {
      e.preventDefault();
      app.moveProvider(i, i + 1);
    }
  }
</script>

<Sheet title={app.editing ? (app.providers.some((p) => p.id === app.editing?.id) ? 'Edit provider' : 'Add provider') : 'Providers'} onclose={close}>
  {#if app.editing}
    {#key app.editing.id}
      <ProviderForm provider={app.editing} ondone={() => (app.editing = null)} />
    {/key}
  {:else}
    <div class="intro small muted">
      <p>
        The order is the priority. Providers that aren't backups share the work. A missing article is tried on the next
        provider in the list.
      </p>
    </div>
    {#if app.providers.length === 0}
      <p class="empty">No providers yet.</p>
    {:else}
      <ol class="list" aria-label="Providers in priority order">
        {#each app.providers as p, i (p.id)}
          <li
            class:over={over === i && dragging !== i}
            class:off={!p.enabled}
            draggable="true"
            ondragstart={(e) => {
              dragging = i;
              e.dataTransfer?.setData('text/plain', String(i));
            }}
            ondragover={(e) => {
              e.preventDefault();
              over = i;
            }}
            ondragleave={() => (over = over === i ? null : over)}
            ondrop={(e) => {
              e.preventDefault();
              drop(i);
            }}
            ondragend={() => {
              dragging = null;
              over = null;
            }}
          >
            <span class="handle" aria-hidden="true">
              <svg width="10" height="14" viewBox="0 0 10 14"
                ><g fill="currentColor"
                  ><circle cx="2" cy="2" r="1.2" /><circle cx="8" cy="2" r="1.2" /><circle cx="2" cy="7" r="1.2" /><circle
                    cx="8"
                    cy="7"
                    r="1.2"
                  /><circle cx="2" cy="12" r="1.2" /><circle cx="8" cy="12" r="1.2" /></g
                ></svg
              >
            </span>
            <button
              class="row"
              onclick={() => (app.editing = { ...p })}
              onkeydown={(e) => keymove(e, i)}
              aria-label={`${p.name || p.host}, priority ${i + 1}. Edit. Alt plus arrow keys to reorder.`}
            >
              <span class="name">{p.name || p.host}</span>
              <span class="meta small muted">
                {#if p.name}<span class="mono">{p.host}</span>{/if}
                <span class="mono">{p.connections} conn</span>
                {#if p.backup}<span>Backup</span>{/if}
                {#if !p.enabled}<span>Off</span>{/if}
              </span>
            </button>
          </li>
        {/each}
      </ol>
    {/if}
    <button class="btn primary add" onclick={() => (app.editing = newProvider())}>Add provider</button>
  {/if}
</Sheet>

<style>
  .intro {
    margin-bottom: 16px;
  }
  .empty {
    color: var(--muted);
    margin-bottom: 16px;
  }
  .list {
    list-style: none;
    margin: 0 0 16px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    overflow: hidden;
  }
  li {
    display: flex;
    align-items: center;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
  li:first-child {
    border-top: 0;
  }
  li.over {
    box-shadow: inset 0 2px 0 var(--fg);
  }
  li.off .name {
    color: var(--muted);
  }
  .handle {
    flex: none;
    padding: 0 6px 0 12px;
    color: var(--muted);
    cursor: grab;
    display: flex;
  }
  .row {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 2px;
    text-align: left;
    padding: 12px 14px 12px 6px;
    background: none;
    border: 0;
    cursor: pointer;
  }
  .row:hover {
    background: var(--surface);
  }
  .row:focus-visible {
    outline-offset: -2px;
  }
  .name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
  }
  .add {
    width: 100%;
  }
</style>
