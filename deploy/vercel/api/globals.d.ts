// Ambient declarations for the Vercel Edge handler.
//
// - `process.env` is polyfilled by Vercel's Edge Runtime (build-injected env
//   vars), but @types/node is intentionally absent (this is an Edge, not Node,
//   function), so declare the narrow slice we read.
// - `*.wasm?module` is a Vercel Edge import suffix that yields a
//   `WebAssembly.Module`; give it a type so the static import type-checks.

declare const process: { env: Record<string, string | undefined> };

declare module "*.wasm?module" {
  const mod: WebAssembly.Module;
  export default mod;
}
