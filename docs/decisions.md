# Decisions

Where the build deviates from, or fills a gap in, the original plan, the
reason is recorded here.

## Engine

- **`rustls-pki-types` `web` feature on wasm32.** rustls' `std` build calls
  `UnixTime::now()`, which doesn't exist on `wasm32-unknown-unknown` without
  this feature. Certificate checks still use our own `TimeProvider` backed by
  `Date.now()` (`crates/engine/src/tls.rs`).
- **`getrandom`.** The ring provider uses getrandom 0.2 with its `js` feature
  (enabled by ring's `wasm32_unknown_unknown_js`). The `getrandom_backend`
  cfg for getrandom 0.3+ is set anyway in `.cargo/config.toml` and the build
  script so a future upgrade doesn't break the build.
- **`Ready` vs `AuthOk`.** `Ready` fires when the server's greeting arrives
  (TLS done, server accepts commands); `AuthOk` follows after login, or right
  away when no username is set. The UI uses `Ready` for "Connected in N ms".
- **Status codes.** `502` during login means "too many connections" at most
  providers, so it maps to `Busy` like `400`. `420`, `423` and `451` are
  treated like `430` (missing). Any other unexpected reply closes the
  connection.
- **Request window.** The engine keeps 8 `BODY` commands on the wire; the
  coordinator hands each connection up to 16 so the pipeline never drains
  while a message crosses workers.
- **PAR2 verification cost.** Each file is hashed once: whole-file MD5 plus
  CRC32 per slice. Per-slice MD5 runs only in a second, "deep" pass when the
  file MD5 fails while every CRC matches. This halves the work for the common
  case while still checking slices by CRC32 and MD5 as the spec asks.
- **PAR2 repair memory.** The repairer works on a stripe of every slice at a
  time with a 256 MB budget for the accumulators, so large repairs run in
  several passes instead of running out of memory.
- **Deobfuscation fallback.** When the first article of a file is missing, its
  first 16 KiB are zeros and the PAR2 16 KiB hash can't match. Files that
  aren't matched by hash are matched by exact length when exactly one
  unmatched PAR2 entry has that length.
- **WASM size.** The engine is about 880 KB (about 390 KB gzipped), mostly
  rustls, ring and the Mozilla root store. It loads only when the first NZB is
  opened or a connection is tested, never on page load.
- **simd128.** The release engine is built with `+simd128` and the `simd`
  feature (yEnc fast path). Every current browser supports WebAssembly SIMD.
  A portable 8-byte SWAR fast path is used everywhere else, including native
  tests.

## Relay

- **Docker build cross-compiles with `rust-lld`.** The Dockerfile builds on
  the build platform for each target (`--platform=$BUILDPLATFORM`), linking
  the static musl binary with `rust-lld`. The relay has no C dependencies, so
  this needs no cross toolchain and no QEMU. The release workflow builds the
  binaries the same way.
- **Default allowlist.** The listed domains plus `*.cheapnews.eu`,
  `*.hitnews.com`, `*.usenetprime.com`, `*.supernews.com` and
  `*.highwinds-media.com`. `*.example.com` matches subdomains only, never the
  apex. Self-hosters can set `SPOOL_ALLOW=*`.
- **Client IP behind a proxy.** With `SPOOL_TRUST_PROXY=1` the right-most
  `X-Forwarded-For` entry is used, which is the one our own reverse proxy
  appended.
- **Order of checks.** Origin, host and port are checked first, then limits.
  The relay waits for the TLS ClientHello before resolving and dialling, so a
  plaintext client never causes an outbound connection.
- **One output line.** Startup failures (bad config, port in use) print on the
  same single line the startup message would have used.

## Web app

- **CSP `connect-src`.** The planned policy allowed only `wss:` and local
  `ws:`. Testing a relay reads its `GET /` info over HTTPS, so `https:` and
  local `http:` are allowed too. The page makes no other requests.
- **CSP in builds only.** Vite's dev server injects styles with JavaScript, so
  the `<meta>` CSP is added at build time by a small Vite plugin.
- **Fonts are committed.** Geist and Geist Mono (OFL) are copied from the
  `geist` npm package into `apps/web/public/fonts` by
  `scripts/update-fonts.sh`. Depending on the package directly would pull in
  Next.js and React as peer dependencies.
- **Workers start on first use.** The coordinator, net, writer and post
  workers are created when the first NZB is opened or a provider is tested,
  keeping the initial page light.
- **Message ports.** The UI creates every worker and wires them with
  `MessageChannel`s. Decoded segments go from a net worker straight to the
  writer as transferable buffers; the coordinator only sees small events.
- **Positional writes.** Chromium writes through one
  `createWritable({ keepExistingData: true })` stream per file, kept open
  until the file completes, so the browser's swap file is copied at most once
  per file. The OPFS fallback uses `createSyncAccessHandle()` in the writer
  worker.
- **File names.** Files start under their NZB subject name and are renamed to
  the yEnc `name=` when they close, unless that would clash with another file.
  PAR2 deobfuscation renames again after download.
- **"Show files".** A web page can't open a folder in the operating system's
  file manager, so "Show files" lists the finished files and the folder they
  were saved in.
- **Staged files (Firefox, Safari).** "Save files" starts one download per
  file from OPFS. The staged copies are removed two minutes later (downloads
  read from them while in progress) and whenever the app starts again, since
  jobs can't be resumed after the tab closes.
- **Recovery volumes.** Volumes are chosen to cover the blocks needed with the
  least download: largest first until covered, then unneeded ones are dropped
  and a smaller single volume can replace the biggest pick. Block counts come
  from `.volNN+MM.par2` names, or are estimated from the size.
- **Queued jobs.** Jobs run one at a time; dropping another NZB while one runs
  queues it.

## Testing

- **mock-nntp generates its CA at startup** with a unique name, so test
  certificates can never chain to an old run's CA. The E2E harness passes the
  CA to the app through `window.__SPOOL_TEST_CA__`, which only builds with the
  `test-ca` cargo feature act on.
- **Missing-article rules** are deterministic (`part % every == offset`,
  scoped to a file name) rather than random ratios, so the overlap between two
  providers, and with it the repair the test needs, is exact.
