#!/usr/bin/env bash
# Build the browser client (warren-client for WebAssembly) into web/pkg, where the
# directory's play page loads it.
#
#   tools/build_web.sh
#
# Needs the wasm32 target (`rustup target add wasm32-unknown-unknown`) and wasm-bindgen-cli
# at the same version as the wasm-bindgen crate (`cargo install wasm-bindgen-cli --version X`).
# Without the target installed, it builds the standard library from source instead (needs the
# rust-src component, or the library sources linked into the toolchain).
set -euo pipefail
cd "$(dirname "$0")/.."
export RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
args=(--profile wasm-release --target wasm32-unknown-unknown -p warren-client --lib)
if rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
  cargo build "${args[@]}"
else
  RUSTC_BOOTSTRAP=1 cargo build "${args[@]}" -Zbuild-std=std,panic_abort
fi
mkdir -p web/pkg
wasm-bindgen --target web --no-typescript --out-dir web/pkg target/wasm32-unknown-unknown/wasm-release/warren_client.wasm
if command -v wasm-opt >/dev/null; then
  wasm-opt -Os -o web/pkg/warren_client_bg.wasm web/pkg/warren_client_bg.wasm
fi
# served pre-compressed by the directory (about a quarter of the size)
for f in web/pkg/warren_client_bg.wasm web/pkg/warren_client.js; do
  gzip -9 -k -f "$f"
  if command -v brotli >/dev/null; then brotli -f -q 9 -o "$f.br" "$f"; fi
done
ls -la web/pkg
