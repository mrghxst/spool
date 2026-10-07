<script lang="ts">
  import { hostMatches } from '../core/relay';
  import { app } from '../state.svelte';
  import type { Provider } from '../types';

  let { provider, ondone }: { provider: Provider; ondone: () => void } = $props();

  // Edit a copy; save on submit.
  // svelte-ignore state_referenced_locally
  let p = $state<Provider>({ ...provider });
  let testing = $state(false);
  let result = $state<{ ok: boolean; message: string } | null>(null);
  let touched = $state(false);

  const isNew = !app.providers.some((x) => x.id === provider.id);
  const hostValid = $derived(/^[a-z0-9.-]+$/i.test(p.host.trim()) && p.host.trim().length > 2);
  const portValid = $derived(Number.isInteger(p.port) && p.port > 0 && p.port < 65536);
  const valid = $derived(hostValid && portValid && p.connections >= 1 && p.connections <= 50);
  const relayBlocks = $derived(
    app.relayInfo && hostValid
      ? !app.relayInfo.allow.some((a) => hostMatches(a, p.host)) || !app.relayInfo.ports.includes(p.port)
      : false,
  );

  async function test() {
    if (testing) return;
    touched = true;
    if (!valid) return;
    testing = true;
    result = null;
    try {
      result = await app.testProvider($state.snapshot(p) as Provider);
    } catch (e) {
      result = { ok: false, message: (e as Error).message };
    } finally {
      testing = false;
    }
  }

  function save(e: SubmitEvent) {
    e.preventDefault();
    touched = true;
    if (!valid) return;
    app.upsertProvider({
      ...($state.snapshot(p) as Provider),
      host: p.host.trim().toLowerCase(),
      name: p.name.trim(),
      username: p.username.trim(),
    });
    ondone();
  }

  function remove() {
    app.removeProvider(p.id);
    ondone();
  }
</script>

<form onsubmit={save} novalidate>
  <label class="field">
    <span>Server</span>
    <input
      class="input mono"
      bind:value={p.host}
      placeholder="news.example.com"
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      inputmode="url"
      required
      aria-invalid={touched && !hostValid}
    />
  </label>
  {#if touched && !hostValid}
    <p class="small danger">Enter the server name, for example news.example.com.</p>
  {/if}
  {#if relayBlocks}
    <p class="small muted">
      Your relay doesn't list this server or port, so it will refuse the connection. Pick another relay in Settings, or run
      your own.
    </p>
  {/if}
  <div class="row">
    <label class="field">
      <span>Port</span>
      <input class="input mono" type="number" min="1" max="65535" bind:value={p.port} aria-invalid={touched && !portValid} />
    </label>
    <label class="field">
      <span>Connections</span>
      <input class="input mono" type="number" min="1" max="50" bind:value={p.connections} />
    </label>
  </div>
  <label class="field">
    <span>Username</span>
    <input class="input" bind:value={p.username} autocomplete="off" autocapitalize="off" spellcheck="false" />
  </label>
  <label class="field">
    <span>Password</span>
    <input class="input" type="password" bind:value={p.password} autocomplete="new-password" />
  </label>
  <label class="field">
    <span>Name (optional)</span>
    <input class="input" bind:value={p.name} placeholder="Shown in the provider list" />
  </label>
  <label class="check">
    <input type="checkbox" bind:checked={p.backup} />
    <span>
      Use only for missing articles
      <span class="small muted block">Connects only when another provider is missing an article.</span>
    </span>
  </label>
  <label class="check">
    <input type="checkbox" bind:checked={p.enabled} />
    <span>Enabled</span>
  </label>

  <div class="test">
    <button type="button" class="btn" onclick={test} aria-disabled={testing}>
      {testing ? 'Testing connection…' : 'Test connection'}
    </button>
    {#if result}
      <p class="small" class:danger={!result.ok} role="status">{result.message}</p>
    {/if}
  </div>

  <div class="actions">
    {#if !isNew}
      <button type="button" class="btn ghost danger-text" onclick={remove}>Delete provider</button>
    {/if}
    <span class="grow"></span>
    <button type="button" class="btn" onclick={ondone}>Cancel</button>
    <button type="submit" class="btn primary">Save provider</button>
  </div>
</form>

<style>
  form {
    display: grid;
    gap: 16px;
  }
  .row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .block {
    display: block;
  }
  .test {
    display: grid;
    gap: 8px;
    justify-items: start;
    padding-top: 4px;
  }
  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    padding-top: 8px;
    border-top: 1px solid var(--border);
    padding-top: 16px;
  }
  .grow {
    flex: 1;
  }
</style>
