#!/usr/bin/env bash
# Regenerate the wasm-bindgen `--target deno` glue for the EdgeOne Pages edge
# function entry, then patch the glue's loader tail from the
# streaming-fetch-from-URL form to a base64-INLINE form so the deployed bundle
# is fully self-contained (no sibling `.wasm` file to fetch at runtime).
#
# EdgeOne Pages Edge Functions are a V8 / Web-Service-Worker isolate that — as
# this spike empirically confirmed — exposes the `WebAssembly` global AND allows
# `WebAssembly.instantiate(bytes, imports)` (runtime byte compilation from a
# buffer). That is the SAME capability tier as Netlify / Supabase / Deno, so we
# reuse the deno-target glue + the base64-inline trick rather than the
# static-`?module`-import model that Vercel / Cloudflare require.
#
# Run from the crate root (/home/linhuan/gproxy/v2):
#   cargo build --lib --target wasm32-unknown-unknown --release
#   bash deploy/eopages/build.sh
set -euo pipefail

CRATE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WASM="$CRATE_ROOT/target/wasm32-unknown-unknown/release/gproxy.wasm"
OUT="$CRATE_ROOT/deploy/eopages/gproxy/edge-functions/_lib"

[ -f "$WASM" ] || { echo "missing $WASM — run cargo build first" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT"
wasm-bindgen --target deno --out-dir "$OUT" "$WASM"

# Emit the base64-inlined wasm module beside the glue.
INLINE="$OUT/gproxy_wasm_inline.ts"
{
  echo "// AUTO-GENERATED — do not edit. Inlined gproxy_bg.wasm (base64) so the"
  echo "// EdgeOne Pages edge-function bundle is self-contained (no static .wasm"
  echo "// file to fetch at runtime). Regenerate with deploy/eopages/build.sh."
  printf 'export const wasmBase64 = "'
  base64 -w0 "$OUT/gproxy_bg.wasm"
  printf '";\n'
} > "$INLINE"

# Rewrite the loader tail of the generated gproxy.js: replace the
# streaming-fetch-from-URL instantiation with the base64-inline form.
perl -0pi -e '
  s{const wasmUrl = new URL\(.gproxy_bg\.wasm., import\.meta\.url\);\n.*?wasm\.__wbindgen_start\(\);}
   {// EdgeOne Pages bundles this module; instantiate from the inlined bytes\n// instead of fetching a sibling URL.\nimport { wasmBase64 } from "./gproxy_wasm_inline.ts";\nconst wasmBytes = Uint8Array.from(atob(wasmBase64), (c) => c.charCodeAt(0));\nconst wasmInstantiated = await WebAssembly.instantiate(wasmBytes, __wbg_get_imports());\nconst wasmInstance = wasmInstantiated.instance;\nconst wasm = wasmInstance.exports;\nwasm.__wbindgen_start();}s
' "$OUT/gproxy.js"

grep -q "instantiate from the inlined bytes" "$OUT/gproxy.js" \
  && echo "patched $OUT/gproxy.js (inline base64 loader)" \
  || { echo "PATCH FAILED — gproxy.js loader tail changed" >&2; exit 1; }

# Drop the now-unused sibling .wasm so it is not uploaded as a static asset.
rm -f "$OUT/gproxy_bg.wasm" "$OUT/gproxy_bg.wasm.d.ts"
echo "done: $OUT"
