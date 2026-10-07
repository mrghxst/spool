<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte';

  let {
    title,
    onclose,
    children,
    footer,
  }: { title: string; onclose: () => void; children: Snippet; footer?: Snippet } = $props();

  let panel: HTMLElement;
  let shown = $state(false);
  let returnFocus: Element | null = null;

  onMount(() => {
    returnFocus = document.activeElement;
    requestAnimationFrame(() => (shown = true));
    void tick().then(() => {
      const first = panel.querySelector<HTMLElement>('input, select, button:not([data-close])');
      (first ?? panel).focus();
    });
    return () => (returnFocus as HTMLElement | null)?.focus?.();
  });

  // Escape closes the sheet wherever focus is (a button that disables
  // itself while working drops focus to the body).
  function windowKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && !e.defaultPrevented) {
      e.preventDefault();
      onclose();
    }
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === 'Tab') {
      // Keep focus inside the sheet.
      const items = [...panel.querySelectorAll<HTMLElement>('a[href], button:not(:disabled), input:not(:disabled), select, [tabindex="0"]')];
      if (!items.length) return;
      const first = items[0];
      const last = items[items.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }
</script>

<svelte:window onkeydown={windowKeydown} />

<div class="overlay" class:shown onclick={onclose} aria-hidden="true"></div>
<div
  class="sheet"
  class:shown
  role="dialog"
  aria-modal="true"
  aria-label={title}
  tabindex="-1"
  bind:this={panel}
  onkeydown={keydown}
>
  <header>
    <h2>{title}</h2>
    <button class="btn ghost icon" data-close aria-label="Close" onclick={onclose}>
      <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"
        ><path d="M3.5 3.5l9 9m0-9l-9 9" stroke="currentColor" stroke-width="1.5" fill="none" /></svg
      >
    </button>
  </header>
  <div class="body">
    {@render children()}
  </div>
  {#if footer}
    <footer>{@render footer()}</footer>
  {/if}
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: var(--overlay);
    opacity: 0;
    transition: opacity 150ms ease-out;
    z-index: 40;
  }
  .overlay.shown {
    opacity: 1;
  }
  .sheet {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(440px, 100vw);
    background: var(--bg);
    border-left: 1px solid var(--border);
    box-shadow: -12px 0 32px rgba(0, 0, 0, 0.08);
    z-index: 50;
    display: flex;
    flex-direction: column;
    transform: translateX(16px);
    opacity: 0;
    transition:
      transform 150ms ease-out,
      opacity 150ms ease-out;
    outline: none;
  }
  .sheet.shown {
    transform: none;
    opacity: 1;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 56px;
    padding: 0 12px 0 20px;
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .body {
    flex: 1;
    overflow-y: auto;
    padding: 20px;
  }
  footer {
    flex: none;
    border-top: 1px solid var(--border);
    padding: 14px 20px;
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    flex-wrap: wrap;
  }
</style>
