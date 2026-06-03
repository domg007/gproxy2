// gproxy v2 — Cloudflare Workers (module worker) entry.
//
// Cloudflare Workers use the SAME static-wasm-module model as Vercel Edge: a
// statically-imported `.wasm` is bundled by wrangler as a `WebAssembly.Module`
// (no `?module` suffix on CF), and runtime byte compilation of arbitrary
// buffers is forbidden. So this entry reuses the wasm-bindgen `--target web`
// glue and hands it the bundled Module:
//
//   import wasmModule from "./_lib/gproxy_bg.wasm";           // WebAssembly.Module
//   import initWasm, { fetch as wasmFetch, init as gproxyInit } from "./_lib/gproxy.js";
//   await initWasm({ module_or_path: wasmModule });           // WebAssembly.instantiate(Module, imports)
//
// The web-target default export (`__wbg_init`) routes a `WebAssembly.Module`
// straight to `WebAssembly.instantiate(module, imports)` (no fetch of the
// .wasm), satisfying the Workers sandbox.
//
// Unlike Vercel (process.env) / Netlify (Netlify.env), a module worker receives
// its bindings via the `env` ARGUMENT of `fetch(request, env, ctx)` — secrets
// set with `wrangler secret put` and `[vars]` from wrangler.toml both land
// there. So `ensureReady` reads creds from `env`, NOT a global.
//
// Credentials (set with `echo -n "$VALUE" | wrangler secret put NAME`):
//   TURSO_URL, TURSO_TOKEN          (required — libSQL/Turso persistence)
//   UPSTASH_URL, UPSTASH_TOKEN      (optional — Upstash Redis cache)
//
// The generated glue (_lib/gproxy.js + gproxy_bg.wasm + *.d.ts) is gitignored;
// only this file + wrangler.toml + build.sh are hand-written source. Build:
//   cargo build --lib --target wasm32-unknown-unknown --release
//   bash cloudflare/build.sh

import wasmModule from "./_lib/gproxy_bg.wasm";
import initWasm, {
  fetch as wasmFetch,
  init as gproxyInit,
} from "./_lib/gproxy.js";

function reqEnv(env, name) {
  const v = env[name];
  if (!v) {
    throw new Error(`missing required env var: ${name}`);
  }
  return v;
}

function optEnv(env, name) {
  const v = env[name];
  return v && v.length > 0 ? v : undefined;
}

// Instantiate the wasm Module + build the shared AppState exactly once, LAZILY
// on the first request — the worker bindings (`env`) are only populated at
// request time, and the Rust `init` is itself idempotent (first AppState wins).
let ready;

function ensureReady(env) {
  if (!ready) {
    ready = (async () => {
      // Pass the bundled WebAssembly.Module — the web-target loader sends it to
      // WebAssembly.instantiate(module, imports) (no byte compile, no URL fetch).
      await initWasm({ module_or_path: wasmModule });
      await gproxyInit(
        reqEnv(env, "TURSO_URL"),
        reqEnv(env, "TURSO_TOKEN"),
        optEnv(env, "UPSTASH_URL"),
        optEnv(env, "UPSTASH_TOKEN"),
      );
    })();
  }
  return ready;
}

export default {
  async fetch(request, env, _ctx) {
    await ensureReady(env);
    // The wasm router matches bare paths (`/healthz`, `/version`); the worker
    // receives the original request URL unchanged, so paths pass straight through.
    return wasmFetch(request);
  },
};
