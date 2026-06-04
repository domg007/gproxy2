# Alibaba Cloud ESA — WASM Feasibility Spike

**Date:** 2026-06-04
**Product:** Alibaba Cloud ESA Functions and Pages (`esa-cli` v1.0.10).
**Question:** Can the gproxy v2 wasm edge build run on ESA?

## STATUS: `LOCAL_RUNTIME_WASM_OK_REMOTE_BLOCKED`

ESA's local edgeworker2 runtime can run a minimal WebAssembly probe:

```text
GET /healthz  -> 200 hello-esa
GET /wasmtest -> 200 wasm-ok value=5 ms=0
```

The remote ESA deployment path was not enough to prove real edge execution yet.
The CLI accepted and activated code versions, but the reachable endpoints did
not enter the function:

- Existing project `gproxy-aliyun` had production version `1780475140004460289`
  and staging probe version `1780545231203006909`.
- New temporary project `gproxy-aliyun-wasm-spike` deployed staging version
  `1780545270201582397` and production version `1780545448906456479`.
- Default `*.er.aliyun-esa.net` access returned `401 Authorization Required` or
  `582 Version retrieval failed`; no handler body was returned.
- A narrow route `www.lin.pub/gproxy-esa-wasm/*` reached the ESA route path but
  returned HTTP/2 `INTERNAL_ERROR` / HTTP/1.1 empty reply.
- Temporary routes and the temporary project were deleted after the test. The
  original `gproxy-aliyun` project was left in place.

Conclusion: do not mark ESA as a real edge-verified gproxy target yet. The
runtime shape is promising, but the public/default domain and route path needs
to be solved before testing the real gproxy wasm package.

## Probe

The minimal probe used `export default { fetch(request) { ... } }`, which is the
entry shape expected by `esa-cli` and the local runtime:

```js
const WASM_B64 = "AGFzbQEAAAABBwFgAn9/AX8DAgEABwcBA2FkZAAACgkBBwAgACABags=";

export default {
  async fetch(request) {
    const url = new URL(request.url);
    if (url.pathname === "/healthz") {
      return new Response("hello-esa\n", {
        headers: { "content-type": "text/plain" },
      });
    }

    const started = Date.now();
    const bytes = Uint8Array.from(atob(WASM_B64), (c) => c.charCodeAt(0));
    const instance = await WebAssembly.instantiate(bytes, {});
    const value = instance.instance.exports.add(2, 3);
    return new Response(`wasm-ok value=${value} ms=${Date.now() - started}\n`, {
      headers: { "content-type": "text/plain" },
    });
  },
};
```

## Commands

```bash
npx -y esa-cli@1.0.10 login \
  --access-key-id "$ALIBABA_CLOUD_ACCESS_KEY_ID" \
  --access-key-secret "$ALIBABA_CLOUD_ACCESS_KEY_SECRET"

npx -y esa-cli@1.0.10 deploy \
  --environment staging \
  --description "gproxy minimal wasm probe"

npx -y esa-cli@1.0.10 deploy \
  --environment production \
  --description "gproxy minimal wasm probe production"

npx -y esa-cli@1.0.10 dev src/wasmtest.js --port 18081 --skip-update-check
```

Cleanup performed:

```bash
npx -y esa-cli@1.0.10 route delete gproxy-esa-wasm-www-path
npx -y esa-cli@1.0.10 route delete gproxy-esa-wasm-spike
npx -y esa-cli@1.0.10 project delete gproxy-aliyun-wasm-spike
```
