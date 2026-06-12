// gproxy v2 — Vercel Edge Function entry.
//
// Vercel's Edge Runtime supports `WebAssembly.instantiate()` ONLY with a
// `WebAssembly.Module` supplied via a static `?module` import — it forbids
// compiling raw bytes/buffers at runtime (unlike Supabase/Netlify, where the
// base64-inline `WebAssembly.instantiate(bytes, …)` trick works). So instead of
// the `deno`-target glue, this entry uses the wasm-bindgen `--target web` glue
// and hands it the statically-imported Module:
//
//   import wasmModule from "./_lib/gproxy_bg.wasm?module";   // WebAssembly.Module
//   import initWasm, { fetch as wasmFetch, init as gproxyInit } from "./_lib/gproxy.js";
//   await initWasm({ module_or_path: wasmModule });          // WebAssembly.instantiate(Module, imports)
//
// The web-target default export (`__wbg_init`) routes a `WebAssembly.Module`
// straight to `WebAssembly.instantiate(module, imports)` (no fetch of the
// .wasm), satisfying Vercel's "no runtime byte compilation" rule.
//
// wasm-fetch: the crate never calls the host `fetch` directly — wasm-bindgen
// generates JS glue (`arg0.fetch(arg1)`) that calls the global `fetch`, so any
// fetch the Rust side performs runs through JS. `/healthz` and `/version` do no
// fetch at request time. `init` does no fetch when an Upstash cache is
// configured (LibsqlCache.connect's CREATE-TABLE fetch is only taken on the
// libSQL kv-cache fallback), so UPSTASH_URL/TOKEN should be set on the project.
//
// Credentials are read from the Edge env (`process.env`) at first request —
// NEVER hard-coded. Set them with `vercel env add … production`:
//   TURSO_URL, TURSO_TOKEN          (required — libSQL/Turso persistence)
//   UPSTASH_URL, UPSTASH_TOKEN      (optional — Upstash Redis cache)
//   GPROXY_MASTER_KEY               (optional — unseals encrypted stored
//                                    secrets; absent → plaintext mode)
//
// The generated glue (_lib/gproxy.js + gproxy_bg.wasm + *.d.ts) is gitignored;
// only this file + vercel.json are hand-written source. Build from the crate
// root, then run vercel from deploy/vercel/:
//   cargo build --lib --target wasm32-unknown-unknown --release --no-default-features --features edge
//   wasm-bindgen --target web --out-dir deploy/vercel/api/_lib \
//     target/wasm32-unknown-unknown/release/gproxy.wasm

// `?module` is a Vercel Edge import suffix that yields a `WebAssembly.Module`
// (typed in ./globals.d.ts).
import wasmModule from "./_lib/gproxy_bg.wasm?module";
import initWasm, {
  fetch as wasmFetch,
  init as gproxyInit,
} from "./_lib/gproxy.js";

export const config = { runtime: "edge" };

function reqEnv(name: string): string {
  const v = process.env[name];
  if (!v) {
    throw new Error(`missing required env var: ${name}`);
  }
  return v;
}

function optEnv(name: string): string | undefined {
  const v = process.env[name];
  return v && v.length > 0 ? v : undefined;
}

// Instantiate the wasm Module + build the shared AppState exactly once, LAZILY
// on the first request. Doing it at module scope risks running during Vercel's
// build-time module evaluation, where the storage env vars are absent.
let ready: Promise<void> | undefined;

function ensureReady(): Promise<void> {
  if (!ready) {
    ready = (async () => {
      // Pass the statically-imported WebAssembly.Module — the web-target loader
      // sends it to WebAssembly.instantiate(module, imports) (no byte compile).
      await initWasm({ module_or_path: wasmModule });
      await gproxyInit(
        reqEnv("TURSO_URL"),
        reqEnv("TURSO_TOKEN"),
        optEnv("UPSTASH_URL"),
        optEnv("UPSTASH_TOKEN"),
        optEnv("GPROXY_MASTER_KEY"),
      );
    })();
  }
  return ready;
}

export default async function handler(req: Request): Promise<Response> {
  await ensureReady();
  // A `vercel.json` rewrite maps every path onto this function while preserving
  // the original request URL, so `/healthz` / `/version` reach the wasm router
  // unchanged — no synthetic function-name prefix to strip.
  return wasmFetch(req);
}
