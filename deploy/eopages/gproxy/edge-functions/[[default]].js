// gproxy v2 — EdgeOne Pages Edge Function entry (catch-all).
//
// EdgeOne Pages routes by directory: `[[default]].js` is the multi-level
// catch-all, so EVERY path (/healthz, /version, …) reaches this handler. The
// wasm router matches bare paths, and EdgeOne passes the original request URL
// through unchanged, so no synthetic prefix to strip.
//
// EdgeOne Pages Edge Functions are a V8 / Web-Service-Worker isolate. This
// spike empirically confirmed the isolate exposes `WebAssembly` and allows
// `WebAssembly.instantiate(bytes, imports)` (buffer compilation), so we reuse
// the wasm-bindgen `--target deno` glue with the wasm INLINED as base64
// (see deploy/eopages/build.sh) — a self-contained bundle, no sibling
// .wasm fetch.
//
// Credentials come from the function context `env` (Pages environment
// variables) — NEVER hard-coded. Set them with `edgeone pages env set`:
//   TURSO_URL, TURSO_TOKEN          (required — libSQL/Turso persistence)
//   UPSTASH_URL, UPSTASH_TOKEN      (optional — Upstash Redis cache)
//
// The generated glue (_lib/gproxy.js + gproxy.d.ts + gproxy_wasm_inline.ts) is
// gitignored — only this file is hand-written source. Build:
//   cargo build --lib --target wasm32-unknown-unknown --release
//   bash deploy/eopages/build.sh
//
// `wasmFetch` is aliased from the wasm `fetch` export so it does not shadow the
// runtime's global `fetch`.
import { fetch as wasmFetch, init } from "./_lib/gproxy.js";

function reqEnv(env, name) {
  const v = env && env[name];
  if (!v) {
    throw new Error(`missing required env var: ${name}`);
  }
  return v;
}

function optEnv(env, name) {
  const v = env && env[name];
  return v && v.length > 0 ? v : undefined;
}

// Build the shared AppState exactly once, LAZILY on the first request — the
// function `env` is only populated at invocation time, and the Rust `init` is
// idempotent (first AppState wins). The promise is memoised at module scope.
let initialised;

function ensureInit(env) {
  if (!initialised) {
    initialised = init(
      reqEnv(env, "TURSO_URL"),
      reqEnv(env, "TURSO_TOKEN"),
      optEnv(env, "UPSTASH_URL"),
      optEnv(env, "UPSTASH_TOKEN"),
    );
  }
  return initialised;
}

export async function onRequest(context) {
  await ensureInit(context.env);
  return wasmFetch(context.request);
}
