#!/usr/bin/env bash
# Regenerate the wasm-bindgen `--target web` glue for the Vercel Edge entry,
# then patch the glue so Vercel's Edge bundler does not try to bundle the .wasm
# as a "blob asset" (which the Edge Runtime rejects with
# "referencing unsupported modules: vc-blob-asset:gproxy_bg.wasm").
#
# The handler (api/index.ts) ALWAYS passes the statically `?module`-imported
# WebAssembly.Module to the loader, so the loader's URL-fetch fallback
# (`new URL('gproxy_bg.wasm', import.meta.url)`) is dead code at runtime — but
# its mere presence makes Vercel's static analyzer pull the .wasm into the
# bundle. Replacing that line with a throw removes the offending reference.
#
# Run from the crate root (/home/linhuan/gproxy/v2):
#   cargo build --lib --target wasm32-unknown-unknown --release
#   bash vercel/build.sh
set -euo pipefail

CRATE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WASM="$CRATE_ROOT/target/wasm32-unknown-unknown/release/gproxy.wasm"
OUT="$CRATE_ROOT/vercel/api/_lib"

[ -f "$WASM" ] || { echo "missing $WASM — run cargo build first" >&2; exit 1; }

rm -rf "$OUT"
wasm-bindgen --target web --out-dir "$OUT" "$WASM"

# Drop the URL-fetch fallback so Vercel does not bundle the .wasm as a blob.
perl -0pi -e \
  "s/module_or_path = new URL\('gproxy_bg\.wasm', import\.meta\.url\);/throw new Error('pass the WebAssembly.Module explicitly (Vercel Edge: no URL fetch of the .wasm)');/" \
  "$OUT/gproxy.js"

grep -q "no URL fetch of the .wasm" "$OUT/gproxy.js" \
  && echo "patched $OUT/gproxy.js (removed gproxy_bg.wasm URL fallback)" \
  || { echo "PATCH FAILED — gproxy.js loader tail changed" >&2; exit 1; }
