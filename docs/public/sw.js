// The app used to live here and registered a service worker at this path.
// It moved to https://spool.itsanon.com; this replacement removes the old
// worker and its caches, then reloads open pages so they get the docs.
self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => {
  e.waitUntil(
    (async () => {
      for (const k of await caches.keys()) if (k.startsWith('spool-')) await caches.delete(k);
      await self.registration.unregister();
      for (const c of await self.clients.matchAll({ type: 'window' })) c.navigate(c.url);
    })(),
  );
});
