#!/usr/bin/env bash
# Build the browser client (vox-client for WebAssembly) into apps/site/public/pkg, where the
# site's build copies it to dist/pkg: the play page and the front page's live view load it.
#
#   tools/build_web.sh               then: (cd apps/site && bun run build)
#   OUT=web/pkg tools/build_web.sh   for the old pages in web/
#
# Needs the wasm32 target (`rustup target add wasm32-unknown-unknown`) and wasm-bindgen-cli
# at the same version as the wasm-bindgen crate (`cargo install wasm-bindgen-cli --version X`).
# Without the target installed, it builds the standard library from source instead (needs the
# rust-src component, or the library sources linked into the toolchain).
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${OUT:-apps/site/public/pkg}"
# cargo install puts wasm-bindgen here, which isn't always on PATH
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
args=(--profile wasm-release --target wasm32-unknown-unknown -p vox-client --lib)
if rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
  cargo build "${args[@]}"
else
  RUSTC_BOOTSTRAP=1 cargo build "${args[@]}" -Zbuild-std=std,panic_abort
fi
mkdir -p "$OUT"
wasm-bindgen --target web --no-typescript --out-dir "$OUT" target/wasm32-unknown-unknown/wasm-release/vox_client.wasm
if command -v wasm-opt >/dev/null; then
  wasm-opt -Os -o "$OUT/vox_client_bg.wasm" "$OUT/vox_client_bg.wasm"
fi
# served pre-compressed by the directory (about a quarter of the size)
for f in "$OUT/vox_client_bg.wasm" "$OUT/vox_client.js"; do
  gzip -9 -k -f "$f"
  if command -v brotli >/dev/null; then brotli -f -q 9 -o "$f.br" "$f"; fi
done
ls -la "$OUT"
