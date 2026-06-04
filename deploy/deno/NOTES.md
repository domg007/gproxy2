# Deno Deploy — WASM Deployment Notes

**Date:** 2026-06-04
**Product:** Deno Deploy.
**Question:** Can the existing `deploy/deno/main.ts` entry be deployed and
verified?

## STATUS: `BLOCKED_ON_NEW_DENO_DEPLOY_APP`

The wasm package builds locally:

```text
cargo build --lib --target wasm32-unknown-unknown --release
wasm-bindgen --target deno --out-dir pkg target/wasm32-unknown-unknown/release/gproxy.wasm
pkg/gproxy_bg.wasm: 412312 bytes
```

The old Deploy Classic path is blocked before code execution. `deployctl`
accepted the token but found no existing `gproxy-deno` project; when it tried to
create one, the API rejected it:

```text
Project 'gproxy-deno' not found. Creating...
error: APIError: New project creation is disabled. Deno Deploy Classic is being
sunset on July 20, 2026. Please migrate to the new Deno Deploy platform.
```

Deno 2.8.2 was installed locally, but `deno deploy --help` could not fetch its
JSR helper in this environment:

```text
error: Import 'https://jsr.io/@deno/deploy/meta.json' failed: 403 Forbidden
```

Conclusion: do not treat this as a gproxy wasm failure. The next step is account
setup on the new Deno Deploy platform, then update this deployment entry for the
new `deno deploy` flow.

## What The User Needs To Do

1. Open `https://console.deno.com`.
2. Create a new Deno Deploy organization.
3. Create an app for this project, for example `gproxy-deno`.
4. Add production environment variables:
   - `TURSO_URL`
   - `TURSO_TOKEN`
   - `UPSTASH_URL`
   - `UPSTASH_TOKEN`
5. Provide the new app/org/token details, or log the local CLI into that new
   account so Codex can retry from this workspace.

The existing `DENO_DEPLOY_TOKEN` in `.env` is not enough if it only belongs to
Deploy Classic or if the target app does not already exist on the new platform.
