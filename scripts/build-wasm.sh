#!/usr/bin/env bash
# Builds the engine to WebAssembly and generates JS bindings.
#
#   scripts/build-wasm.sh            release engine
#   scripts/build-wasm.sh --test-ca  engine that can trust the mock-nntp CA (E2E only)
#
# Needs: rustup target wasm32-unknown-unknown, wasm-bindgen-cli matching the
# wasm-bindgen crate version, wasm-opt (binaryen), and clang for ring.
set -euo pipefail
cd "$(dirname "$0")/.."

features=simd
out=apps/web/src/lib/engine/pkg
if [[ "${1:-}" == "--test-ca" ]]; then
  features="simd,test-ca"
fi

# RUSTFLAGS replaces the config's target rustflags, so repeat the getrandom cfg.
export RUSTFLAGS='--cfg getrandom_backend="wasm_js" -C target-feature=+simd128'
cargo build -p spool-engine --lib --release --target wasm32-unknown-unknown --features "$features"

rm -rf "$out"
wasm-bindgen --target web --out-dir "$out" --out-name engine \
  target/wasm32-unknown-unknown/release/spool_engine.wasm

if command -v wasm-opt >/dev/null; then
  wasm-opt -O3 \
    --enable-simd --enable-bulk-memory --enable-mutable-globals \
    --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-reference-types --enable-multivalue \
    "$out/engine_bg.wasm" -o "$out/engine_bg.wasm"
else
  echo "warning: wasm-opt not found; skipping optimisation" >&2
fi

# Guard: the release engine must not contain the test-CA hook.
if [[ "$features" != *test-ca* ]] && grep -q add_test_root "$out/engine.js"; then
  echo "error: release engine exports add_test_root" >&2
  exit 1
fi
ls -l "$out"
