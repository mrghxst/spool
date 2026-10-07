#!/usr/bin/env bash
# Installs the engine's build tools into ~/.spool-toolchain on a build host
# that has none of them (Cloudflare Workers Builds, Netlify, a bare
# container). Prints `export` lines to eval:
#
#   eval "$(bash scripts/bootstrap-toolchain.sh)"
#
# Installs only what's missing: Rust with the wasm32 target, wasm-bindgen-cli,
# wasm-opt, and a clang that can target wasm32 (from wasi-sdk) for ring.
# Linux x86_64 only.
set -euo pipefail

WASM_BINDGEN=0.2.129
BINARYEN=version_133
WASI_SDK=34

dir="${SPOOL_TOOLCHAIN_DIR:-$HOME/.spool-toolchain}"
mkdir -p "$dir"
log() { echo "bootstrap: $*" >&2; }
fetch() { curl --proto '=https' --tlsv1.2 -fsSL --retry 3 "$1"; }

export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export PATH="$CARGO_HOME/bin:$dir/bin:$PATH"

if ! command -v cargo >/dev/null; then
  log "installing Rust"
  fetch https://sh.rustup.rs | sh -s -- -y -q --profile minimal --no-modify-path \
    --default-toolchain stable --target wasm32-unknown-unknown >&2
else
  rustup target add wasm32-unknown-unknown >&2
fi

if [[ "$(wasm-bindgen --version 2>/dev/null)" != "wasm-bindgen $WASM_BINDGEN" ]]; then
  log "installing wasm-bindgen $WASM_BINDGEN"
  name="wasm-bindgen-$WASM_BINDGEN-x86_64-unknown-linux-musl"
  fetch "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/$WASM_BINDGEN/$name.tar.gz" \
    | tar xz -C "$dir"
  mkdir -p "$dir/bin"
  mv "$dir/$name/wasm-bindgen" "$dir/bin/"
  rm -rf "${dir:?}/$name"
fi

if ! command -v wasm-opt >/dev/null; then
  log "installing binaryen $BINARYEN"
  fetch "https://github.com/WebAssembly/binaryen/releases/download/$BINARYEN/binaryen-$BINARYEN-x86_64-linux.tar.gz" \
    | tar xz -C "$dir"
  ln -sf "$dir/binaryen-$BINARYEN/bin/wasm-opt" "$dir/bin/wasm-opt"
fi

# ring compiles C for wasm32, which needs clang with the WebAssembly backend.
cc_line=""
if ! (command -v clang >/dev/null && clang --print-targets 2>/dev/null | grep -q wasm32); then
  sdk="$dir/wasi-sdk-$WASI_SDK.0-x86_64-linux"
  if [[ ! -x "$sdk/bin/clang" ]]; then
    log "installing clang from wasi-sdk $WASI_SDK"
    fetch "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-$WASI_SDK/wasi-sdk-$WASI_SDK.0-x86_64-linux.tar.gz" \
      | tar xz -C "$dir" --exclude='*/share/wasi-sysroot' --exclude='*/share/cmake' 
  fi
  cc_line="export CC_wasm32_unknown_unknown='$sdk/bin/clang' AR_wasm32_unknown_unknown='$sdk/bin/llvm-ar'"
fi

echo "export CARGO_HOME='$CARGO_HOME' RUSTUP_HOME='$RUSTUP_HOME' PATH='$PATH'"
[[ -n "$cc_line" ]] && echo "$cc_line"
log "ready: $(rustc --version), $(wasm-bindgen --version), $(wasm-opt --version | head -1)"
