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

## Extraction

- **Our own Emscripten build of libarchive.** `libarchive.js` takes a single
  `File`, so it can't open multi-volume RAR or split 7z. Spool builds
  libarchive 3.8.9 with liblzma (xz 5.8.4), zlib and bzip2
  (`scripts/build-libarchive.sh`) behind a small pull API
  (`tools/archive-wasm/spool_archive.c`). Volumes are mounted with WORKERFS,
  which reads disk-backed `File` objects synchronously inside the worker, and
  opened together with `archive_read_open_filenames`. That handles
  multi-volume RAR4/RAR5 and presents split files (`.7z.001`, `.zip.001`) as
  one seekable stream. Output is written in 1 MB chunks.
- **The WASM build is committed** under `apps/web/src/lib/archive/vendor/`
  (about 370 KB, loaded only at extraction time) with the third-party license
  texts, so CI and contributors don't need Emscripten. The build script pins
  the source tarballs by SHA-256.
- **No decryption.** libarchive can't decrypt RAR, and 7z/zip AES would need a
  crypto library. Encrypted archives are detected, their parts are kept, any
  partial output is removed, and the NZB password is shown.
- **Cleanup** deletes archive parts and PAR2 files only when every archive set
  extracted, and only if no article stayed missing after repair.

## Performance

`bash e2e/run.sh --perf` downloads a 256 MB file (700 KB articles, 16
connections) from mock-nntp through a release relay on the same machine, in
headless Chromium. On a 16-thread desktop (WSL2):

| Net workers | Transfer speed |
| --- | --- |
| 1 | 63 to 66 MB/s |
| 2 | 65 to 75 MB/s |
| 4 | 65 to 74 MB/s |

The same engine, relay and mock server do 315 MB/s on one connection and
1.4 GB/s on 16 when driven natively, so the ceiling above is in the browser.
Instrumenting a single net worker showed the WASM engine (TLS, NNTP, yEnc,
CRC) busy about 43% of the time at 72 MB/s, which puts one worker's capacity
near 170 MB/s. At these speeds the limit is Chromium delivering WebSocket
messages to workers, not the engine. More net workers help when one worker's
CPU is the limit, as on slower laptops. K stays at
`min(hardwareConcurrency, 4)`, and `?e2e&net=N` pins it for benchmarks.

## Testing

- **mock-nntp generates its CA at startup** with a unique name, so test
  certificates can never chain to an old run's CA. The E2E harness passes the
  CA to the app through `window.__SPOOL_TEST_CA__`, which only builds with the
  `test-ca` cargo feature act on.
- **RAR E2E test is skipped in CI.** RAR archives can only be created with
  RARLAB's proprietary `rar` tool, which CI doesn't install. The test
  `4b. multi-volume RAR extracts` runs when `rar` is on the PATH while the
  fixtures are generated, and is skipped otherwise. libarchive's own RAR test
  vectors were considered, but they're intentionally truncated volumes that no
  extractor can unpack completely. Multi-volume 7z extraction is covered in CI.
- **Missing-article rules** are deterministic (`part % every == offset`,
  scoped to a file name) rather than random ratios, so the overlap between two
  providers, and with it the repair the test needs, is exact.
- **The Chromium folder writer is tested on OPFS.** Native pickers can't be
  automated, so `?e2e=fsa` points the `createWritable` writer at an OPFS
  directory handle (Chromium supports both on OPFS). Every other E2E case uses
  the Firefox/Safari path (OPFS sync handles, then "Save files").
- **UI review.** `bash e2e/run.sh --screens` captures every screen in both
  themes and at 360 px, fails on horizontal scroll, and produces the README
  hero. A keyboard test covers focus rings, focus trapping in sheets, Escape,
  and focus returning to the button that opened a sheet.

## Hosting

- `apps/web/.env.production` sets the default relay so every host builds the
  same app; the Pages workflow can still override it with the
  `VITE_DEFAULT_RELAY` repository variable.
- Cloudflare Workers Builds has Node but no Rust or clang. Rather than commit
  build output, `scripts/build-wasm.sh` runs `scripts/bootstrap-toolchain.sh`
  when `WORKERS_CI` (or `CF_PAGES`, or `SPOOL_BOOTSTRAP`) is set. It installs
  a minimal Rust, prebuilt wasm-bindgen and binaryen, and clang from wasi-sdk
  (ring compiles C for wasm32). Tested in a bare `ubuntu:24.04` container.
- The relay doesn't terminate TLS itself. From an https page, browsers only
  allow `ws://` to localhost, so a LAN relay goes behind a reverse proxy with
  a certificate. The settings sheet says so instead of a generic error.
