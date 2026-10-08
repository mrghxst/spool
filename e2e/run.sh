#!/usr/bin/env bash
# End-to-end tests: mock-nntp (4 providers) + spool-relay + a test-CA build
# of the app served by `vite preview`, driven by Playwright (Chromium).
#
#   bash e2e/run.sh            build everything and run the tests
#   bash e2e/run.sh --serve    build and keep the servers running (manual testing)
#   bash e2e/run.sh --perf     throughput benchmark (256 MB through the relay)
#   bash e2e/run.sh --screens  screenshots of every screen in both themes
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
W=e2e/.work
mkdir -p "$W"

[[ "${1:-}" == "--perf" ]] && export SPOOL_PERF=1
[[ "${1:-}" == "--screens" ]] && export SPOOL_SCREENS=1
bash e2e/fixtures.sh >/dev/null
# Release builds: the benchmark should measure the browser, not a debug relay.
cargo build -q --release -p mock-nntp -p spool-relay
bash scripts/build-wasm.sh --test-ca >/dev/null
VITE_DEFAULT_RELAY=ws://127.0.0.1:18080 pnpm --filter @spool/web exec vite build --outDir dist-e2e --emptyOutDir >/dev/null

pids=()
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done; }
trap cleanup EXIT

rm -f "$W/mock/ready.json"
target/release/mock-nntp "$W/mock.json" 2>"$W/mock.log" &
pids+=($!)
SPOOL_LISTEN=127.0.0.1:18080 SPOOL_ALLOW='*' SPOOL_PORTS=15631,15632,15633,15634,15635,15636,15637 SPOOL_ALLOW_PRIVATE=1 \
  target/release/spool-relay 2>"$W/relay.log" &
pids+=($!)
(cd apps/web && exec npx vite preview --outDir dist-e2e --host 127.0.0.1 --port 4173 --strictPort >"$ROOT/$W/preview.log" 2>&1) &
pids+=($!)

for _ in $(seq 1 100); do
  [[ -f "$W/mock/ready.json" ]] && curl -fsS http://127.0.0.1:18080/healthz >/dev/null 2>&1 \
    && curl -fsS http://127.0.0.1:4173/ >/dev/null 2>&1 && break
  sleep 0.2
done
cat "$W/mock/ready.json"; echo

if [[ "${1:-}" == "--screens" ]]; then
  pnpm exec playwright test -c e2e/playwright.screens.config.ts
elif [[ "${1:-}" == "--perf" ]]; then
  pnpm exec playwright test -c e2e/playwright.perf.config.ts
elif [[ "${1:-}" == "--serve" ]]; then
  echo "App: http://127.0.0.1:4173/?e2e  Relay: ws://127.0.0.1:18080  Providers: 127.0.0.1:15631-15634 (spool/secret)"
  wait
else
  pnpm exec playwright test -c e2e/playwright.config.ts "${@}"
fi
