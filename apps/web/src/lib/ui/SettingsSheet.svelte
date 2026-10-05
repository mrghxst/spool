<script lang="ts">
  import { normalizeRelay, relayAllowed } from '../core/relay';
  import { app, canPickFolder, DEFAULT_RELAY } from '../state.svelte';
  import type { Theme } from '../types';
  import Sheet from './Sheet.svelte';

  let relay = $state(app.settings.relay);
  let relayMsg = $state<{ ok: boolean; text: string } | null>(null);
  let testing = $state(false);
  let confirmForget = $state(false);
  let importMsg = $state<{ ok: boolean; text: string } | null>(null);
  let fileInput: HTMLInputElement;

  const relayValid = $derived(relayAllowed(normalizeRelay(relay)));

  async function testRelay() {
    const url = normalizeRelay(relay);
    if (!relayAllowed(url)) {
      relayMsg = { ok: false, text: 'Use a wss:// address, or ws://localhost for a relay on this computer.' };
      return;
    }
    relay = url;
    app.setRelay(url);
    testing = true;
    relayMsg = null;
    const info = await app.checkRelay();
    testing = false;
    relayMsg = info
      ? {
          ok: true,
          text: `Relay ${info.version} works. It allows ${info.allow.length === 1 && info.allow[0] === '*' ? 'any public server' : info.allow.join(', ')} on ${info.ports.length === 1 ? 'port' : 'ports'} ${info.ports.join(', ')}.`,
        }
      : { ok: false, text: app.relayError ?? "Couldn't reach the relay." };
  }

  function saveRelay() {
    const url = normalizeRelay(relay);
    if (url && relayAllowed(url) && url !== app.settings.relay) {
      app.setRelay(url);
      void app.checkRelay();
    }
  }

  function exportSettings() {
    const blob = new Blob([app.exportJson()], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'spool-settings.json';
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 10_000);
  }

  async function importSettings(e: Event) {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (!file) return;
    try {
      app.importJson(await file.text());
      relay = app.settings.relay;
      importMsg = { ok: true, text: `Imported ${app.providers.length} ${app.providers.length === 1 ? 'provider' : 'providers'}.` };
    } catch (err) {
      importMsg = { ok: false, text: (err as Error).message };
    }
    (e.target as HTMLInputElement).value = '';
  }

  async function forget() {
    await app.forgetEverything();
    relay = app.settings.relay;
    confirmForget = false;
    app.sheet = null;
  }

  const themes: { value: Theme; label: string }[] = [
    { value: 'system', label: 'System' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];
</script>

<Sheet title="Settings" onclose={() => (app.sheet = null)}>
  <section>
    <h3>Relay</h3>
    <p class="small muted">
      Your browser can't reach Usenet servers directly. The relay forwards encrypted traffic and can't read it.
    </p>
    <label class="field">
      <span>Relay address</span>
      <input
        class="input mono"
        bind:value={relay}
        onchange={saveRelay}
        placeholder={DEFAULT_RELAY}
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        aria-invalid={!relayValid}
      />
    </label>
    <div class="inline">
      <button class="btn" onclick={testRelay} disabled={testing}>{testing ? 'Testing relay…' : 'Test relay'}</button>
      {#if relay !== DEFAULT_RELAY}
        <button
          class="btn ghost"
          onclick={() => {
            relay = DEFAULT_RELAY;
            saveRelay();
          }}>Use default relay</button
        >
      {/if}
    </div>
    {#if relayMsg}
      <p class="small" class:danger={!relayMsg.ok} role="status">{relayMsg.text}</p>
    {:else if app.relayStatus === 'error'}
      <p class="small danger">{app.relayError}</p>
    {/if}
    <p class="small muted">
      Run your own with <span class="mono">docker run -p 8080:8080 ghcr.io/mrghxst/spool-relay</span> and use
      <span class="mono">ws://localhost:8080</span>.
    </p>
  </section>

  {#if canPickFolder}
    <section>
      <h3>Download folder</h3>
      <p class="small muted">
        {#if app.folderName}
          Files go to <span class="mono">{app.folderName}</span>, in a folder per download.
        {:else}
          You'll choose a folder the first time you download.
        {/if}
      </p>
      <div class="inline"><button class="btn" onclick={() => app.chooseFolder()}>Choose download folder</button></div>
    </section>
  {/if}

  <section>
    <h3>After downloading</h3>
    <label class="check">
      <input
        type="checkbox"
        checked={app.settings.cleanup}
        onchange={(e) => {
          app.settings.cleanup = (e.target as HTMLInputElement).checked;
          app.save();
        }}
      />
      <span
        >Delete archive parts and PAR2 files after a verified extract
        <span class="small muted block">Keeps only the extracted files.</span></span
      >
    </label>
  </section>

  <section>
    <h3>Theme</h3>
    <div class="segmented" role="radiogroup" aria-label="Theme">
      {#each themes as t (t.value)}
        <button
          role="radio"
          aria-checked={app.settings.theme === t.value}
          class:on={app.settings.theme === t.value}
          onclick={() => app.setTheme(t.value)}>{t.label}</button
        >
      {/each}
    </div>
  </section>

  <section>
    <h3>Privacy</h3>
    <label class="check">
      <input
        type="checkbox"
        checked={app.settings.remember}
        onchange={(e) => app.setRemember((e.target as HTMLInputElement).checked)}
      />
      <span
        >Remember on this device
        <span class="small muted block"
          >Off keeps providers and settings in memory only, which suits shared computers. They're gone when you close the
          tab.</span
        ></span
      >
    </label>
    <div class="inline">
      <button class="btn" onclick={exportSettings}>Export settings</button>
      <button class="btn" onclick={() => fileInput.click()}>Import settings</button>
      <input bind:this={fileInput} type="file" accept="application/json,.json" hidden onchange={importSettings} />
    </div>
    <p class="small muted">The export file contains your provider passwords. Keep it private.</p>
    {#if importMsg}
      <p class="small" class:danger={!importMsg.ok} role="status">{importMsg.text}</p>
    {/if}
    {#if confirmForget}
      <div class="confirm" role="alertdialog" aria-label="Forget everything">
        <p class="small">This deletes providers, settings, the folder choice and any staged files from this browser.</p>
        <div class="inline">
          <button class="btn" onclick={() => (confirmForget = false)}>Keep everything</button>
          <button class="btn danger-text" onclick={forget}>Forget everything</button>
        </div>
      </div>
    {:else}
      <div class="inline"><button class="btn danger-text" onclick={() => (confirmForget = true)}>Forget everything</button></div>
    {/if}
  </section>
</Sheet>

<style>
  section {
    display: grid;
    gap: 12px;
    padding-bottom: 24px;
    margin-bottom: 24px;
    border-bottom: 1px solid var(--border);
  }
  section:last-child {
    border-bottom: 0;
    margin-bottom: 0;
  }
  h3 {
    margin: 0;
    font-size: var(--t14);
    font-weight: 600;
  }
  .inline {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .block {
    display: block;
  }
  .segmented {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    padding: 2px;
    gap: 2px;
    justify-self: start;
  }
  .segmented button {
    height: 30px;
    padding: 0 12px;
    border: 0;
    border-radius: 4px;
    background: none;
    cursor: pointer;
    color: var(--muted);
    font-weight: 500;
  }
  .segmented button.on {
    background: var(--surface);
    color: var(--fg);
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .confirm {
    display: grid;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--danger);
    border-radius: var(--r-sheet);
  }
</style>
