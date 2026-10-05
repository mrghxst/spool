import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { app, E2E } from './lib/state.svelte';

mount(App, { target: document.getElementById('app')! });

// Installed PWA: open .nzb files from the operating system.
type LaunchParams = { files: FileSystemFileHandle[] };
const launchQueue = (window as unknown as { launchQueue?: { setConsumer(f: (p: LaunchParams) => void): void } })
  .launchQueue;
launchQueue?.setConsumer(async (params) => {
  const handle = params.files[0];
  if (handle) await app.openNzb(await handle.getFile());
});

if ('serviceWorker' in navigator && import.meta.env.PROD && !E2E) {
  window.addEventListener('load', () => {
    navigator.serviceWorker.register('./sw.js', { scope: './' }).catch(() => {});
  });
}
