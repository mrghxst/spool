<script lang="ts">
  import { app } from '../state.svelte';
  import type { Theme } from '../types';

  const next: Record<Theme, Theme> = { system: 'light', light: 'dark', dark: 'system' };
  const themeLabel: Record<Theme, string> = {
    system: 'Theme: system',
    light: 'Theme: light',
    dark: 'Theme: dark',
  };

  const relayLabel = $derived(
    app.relayStatus === 'ok'
      ? 'Relay connected'
      : app.relayStatus === 'error'
        ? "Relay can't be reached"
        : 'Checking relay',
  );
</script>

<header class="bar">
  <div class="inner">
    <a class="wordmark" href="./" aria-label="Spool home">spool</a>
    <nav aria-label="Main">
      <button
        class="btn ghost relay"
        title={app.relayError ?? relayLabel}
        aria-label={relayLabel}
        onclick={() => (app.sheet = 'settings')}
      >
        <span class="dot" data-status={app.relayStatus} aria-hidden="true"></span>
        <span class="label">Relay</span>
      </button>
      <button class="btn ghost" onclick={() => (app.sheet = 'providers')}>Providers</button>
      <button class="btn ghost" onclick={() => (app.sheet = 'settings')}>Settings</button>
      <button
        class="btn ghost icon"
        aria-label={`${themeLabel[app.settings.theme]}. Switch theme.`}
        title={themeLabel[app.settings.theme]}
        onclick={() => app.setTheme(next[app.settings.theme])}
      >
        {#if app.settings.theme === 'light'}
          <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="8" cy="8" r="3" fill="none" stroke="currentColor" stroke-width="1.5" />
            <path
              d="M8 1v2M8 13v2M1 8h2M13 8h2M3 3l1.4 1.4M11.6 11.6L13 13M3 13l1.4-1.4M11.6 4.4L13 3"
              stroke="currentColor"
              stroke-width="1.5"
            />
          </svg>
        {:else if app.settings.theme === 'dark'}
          <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
            <path d="M13.5 9.5A6 6 0 0 1 6.5 2.5a6 6 0 1 0 7 7z" fill="none" stroke="currentColor" stroke-width="1.5" />
          </svg>
        {:else}
          <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.5" />
            <path d="M8 2a6 6 0 0 1 0 12z" fill="currentColor" />
          </svg>
        {/if}
      </button>
    </nav>
  </div>
</header>

<style>
  .bar {
    position: sticky;
    top: 0;
    z-index: 20;
    height: 56px;
    border-bottom: 1px solid var(--border);
    background: var(--bg);
  }
  .inner {
    max-width: 880px;
    height: 100%;
    margin: 0 auto;
    padding: 0 16px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .wordmark {
    font-size: 20px;
    font-weight: 700;
    letter-spacing: -0.05em;
    text-decoration: none;
    border-radius: 4px;
  }
  nav {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .relay {
    gap: 8px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: 1.5px solid var(--muted);
  }
  .dot[data-status='ok'] {
    background: var(--fg);
    border-color: var(--fg);
  }
  .dot[data-status='error'] {
    background: var(--danger);
    border-color: var(--danger);
  }
  @media (max-width: 480px) {
    .relay .label {
      display: none;
    }
    .relay {
      width: 36px;
      padding: 0;
    }
  }
</style>
