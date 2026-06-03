// Deno harness for the gproxy v2 wasm edge-storage self-test.
//
// Loads the wasm-bindgen `deno`-target glue from ./pkg, reads the four test
// credentials from the environment (sourced from ./.env — never hard-coded
// here), invokes the Rust `storage_selftest` export against the live Turso +
// Upstash endpoints, and prints the per-step summary it returns.
//
// Full reproduction (pkg/ is gitignored, regenerate it first):
//   cargo build --lib --target wasm32-unknown-unknown --release
//   wasm-bindgen --target deno --out-dir pkg \
//     target/wasm32-unknown-unknown/release/gproxy.wasm
//   # The crate also exports a fn named `fetch` (the WinterCG entry point),
//   # which shadows the global `fetch` that wasm-bindgen's deno loader uses to
//   # read the .wasm file, crashing at import ("Cannot access 'wasm' before
//   # initialization"). Force the loader to use the global explicitly:
//   perl -0pi -e \
//     's/instantiateStreaming\(fetch\(wasmUrl\)/instantiateStreaming(globalThis.fetch(wasmUrl)/' \
//     pkg/gproxy.js
//   set -a && source ./.env && set +a
//   deno run --allow-net --allow-env --allow-read selftest.ts

import { storage_selftest } from "./pkg/gproxy.js";

function reqEnv(name: string): string {
  const v = Deno.env.get(name);
  if (!v) {
    console.error(`missing required env var: ${name}`);
    Deno.exit(1);
  }
  return v;
}

const tursoUrl = reqEnv("GPROXY_TEST_TURSO_URL");
const tursoToken = reqEnv("GPROXY_TEST_TURSO_TOKEN");
const upstashUrl = reqEnv("GPROXY_TEST_UPSTASH_URL");
const upstashToken = reqEnv("GPROXY_TEST_UPSTASH_TOKEN");

const summary: string = await storage_selftest(
  tursoUrl,
  tursoToken,
  upstashUrl,
  upstashToken,
);

console.log("=== gproxy v2 edge storage self-test ===");
console.log(summary);
