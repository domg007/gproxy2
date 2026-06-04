# Tencent EdgeOne **Pages** — WASM Edge Function Feasibility Spike

**Branch:** `phase-1k-edgeone-pages`
**Date:** 2026-06-04
**Product:** EdgeOne **Pages** (the full-stack Pages product, with its OWN
`edgeone` npm CLI + Pages API token) — DISTINCT from the EdgeOne CDN `teo`
2022-09-01 API explored in the earlier `phase-1i-edgeone-spike`, which found
the account had no `teo` zone.
**Question:** Can gproxy v2's WebAssembly edge build deploy + run on EdgeOne Pages?

## STATUS: `TRIVIAL_LIVE_BUT_WASM_FAILED`

EdgeOne Pages edge functions **DO run WebAssembly** — a minimal module
instantiates and executes live (curl → `5`). But gproxy's **real ~612 KB**
wasm module **cannot be instantiated**: `WebAssembly.instantiate()` of it is
hard-killed by the edge-function execution limit at a fixed **~15.0 s** (TCP
reset, zero bytes), even though the SAME bytes `WebAssembly.compile()` in 3 ms.
So WASM-on-Pages is real but the gproxy module is **too large to instantiate
within the isolate's budget**.

---

## Tooling / CLI

- **CLI:** `edgeone` **v1.5.6** (`npm i -g edgeone`, node v22.20.0).
  Commands: `login | whoami | switch | logout | pages {init,dev,
  generate-routes,env,link,deploy}`.
- **Auth (non-interactive): the CLI reads `EDGEONE_PAGES_API_TOKEN` from the
  environment directly** — no `edgeone login` needed. Proven in a clean env
  (`env -i` carrying ONLY that var): `edgeone whoami` → authenticated as
  account `100016841661` / APPID `1304789703`. `edgeone login --token …` is NOT
  required; `pages deploy/link/env` also accept `-t/--token`.
- **Deploy URLs are `*.edgeone.run`** (not `*.edgeone.app`), and are
  **preview/preset deployments gated by a one-time `?eo_token=…&eo_time=…`**
  query pair that 302-redirects and sets `eo_token`/`eo_time` HttpOnly cookies
  (Max-Age 10800 s = 3 h). So every curl must carry the query pair AND follow
  the redirect with a cookie jar:
  `curl -L -c jar -b jar "https://<host>/<path>?eo_token=…&eo_time=…"`.
  Without it: `HTTP 401  X-EOP-MSG: eo_time missing`.
- **DNS is clean** (no hijack): `*.edgeone.run` resolves to a real EdgeOne edge
  IP `61.241.178.245` (`pages.openedge.sched.txdl1.com`); no `--resolve` needed.

## Function convention (learned from `edgeone pages init`)

- Edge functions live under **`edge-functions/`** (NOT `functions/`). Each file
  is a route by its path: `edge-functions/healthz.js` → `/healthz`. Subdirs are
  allowed; a `_lib/` subdir holds importable modules (its files are not served
  as functions). Catch-all is `edge-functions/[[default]].js`.
- Handler is the exported **`onRequest(context)`** returning a `Response`
  (Edge Functions do NOT support `addEventListener`). `context` = `{ request,
  env, params, uuid, waitUntil }`; **env vars come from `context.env`**.
- `generate-routes` only emits a static `routes.json` (filesystem handler); it
  is NOT how functions register — the platform auto-discovers `edge-functions/`
  during the build. (It reports "No server-handler detected … pure project"
  even for projects that DO have working functions, so it is a red herring.)

## Step 1 — Auth: **OK** (see above).

## Step 2 — Trivial deploy: **LIVE** ✅

`edgeone pages deploy deploy/eopages/trivial --name gproxy-spike -t <TOK>
-e production` → project `pages-duubpxy7tneq`, host
`gproxy-spike-te2iwbpy.edgeone.run`.

```
GET /healthz  -> 200  "hello-edgeone-pages"     (edge-functions/healthz.js, onRequest)
GET /         -> 200  <static index.html>
```

## Step 3 — WASM instantiate (minimal module): **WORKS** ✅

`edge-functions/wasmtest.js` decodes a 41-byte base64-embedded wasm module
(exports `add(i32,i32)->i32`) and runs
`WebAssembly.instantiate(bytes, {}); exports.add(2,3)`:

```
GET /wasmtest -> 200  "5"
```

**EdgeOne Pages edge functions expose the `WebAssembly` global AND allow
`WebAssembly.instantiate(bytes, imports)` (runtime byte/buffer compilation)** —
the same capability tier as Netlify / Supabase / Deno, and MORE permissive than
Vercel / Cloudflare (which forbid buffer instantiation and require a static
`?module` import). So the base64-inline glue approach is the right model.

## Step 4 — Real gproxy wasm: **BLOCKED** ❌ (instantiate killed at ~15 s)

Built the deno-target glue + base64-inlined the 612 KB `gproxy_bg.wasm`
(`deploy/eopages/build.sh`), wrote a catch-all `[[default]].js` that
lazy-`init`s from `context.env` and routes to `wasmFetch`. Set the four storage
vars with `edgeone pages env set` (after `edgeone pages link gproxy-v2`), then
deployed `gproxy-v2` (`pages-zsrplszrfd5s`).

`/healthz` + `/version` fell through to the static index → **404 from the
function** at first; isolation testing (a separate `gproxy-iso` project)
pinpointed the cause — it is NOT routing and NOT the `init` network path. The
following probes (each a separate `onRequest` in the SAME project, sharing the
SAME `_lib/` glue) localize it precisely:

| probe | what it does | result |
|---|---|---|
| `/probe` | plain `onRequest`, no imports | **200** `probe-plain-ok` (functions DO register) |
| `/probedecode` | import 800 KB base64 + `atob`→bytes, **no wasm** | **200** `decode-ok bytes=612854 ms=108` |
| `/probecompile` | decode + **`WebAssembly.compile(bytes)`** (no instantiate) | **200** `compile-ok compile_ms=3 exports=14` |
| `/probeinst` | decode + **`WebAssembly.instantiate(bytes, imports)`** (glue) | **TCP reset @ ~15.05 s** |
| `/probesplit` | instantiate with `__wbindgen_start()` wrapped in try/catch (never throws) | **TCP reset @ ~15.09 s** |
| `/probewasm` | full `init()` + `wasmFetch('/version')` | **TCP reset @ ~15.10 s** |

Conclusion chain:
- A 41-byte module instantiates instantly (`5`). ✅
- The 612 KB module **decodes** fine (108 ms) and **compiles** fine (3 ms). ✅
- The 612 KB module **`instantiate()` is hard-killed at a fixed ~15.0 s**, with
  or without the Rust start fn, and emits **no response headers** (connection
  held open exactly 15 s, then RST, 0 bytes). ❌
- Locally in node v22 the identical bytes `instantiate` in ~0 ms with 51 stub
  imports — so the module is cheap on standard V8; the kill is EdgeOne-specific.

This is an EdgeOne Pages edge-function **resource limit on wasm instantiation**
(the 15.0 s constant = a fixed execution/CPU-time kill; the isolate is
terminated mid-instantiate, presumably during full codegen/linking of the
612 KB module — `compile` appears to be lazy/streaming, deferring the real cost
to `instantiate`). NOT a missing-`WebAssembly` problem, NOT a routing problem,
NOT the Turso/Upstash init.

## Exact CLI commands (secrets redacted)

```bash
npm i -g edgeone                                   # v1.5.6
env -i PATH=$PATH HOME=$HOME EDGEONE_PAGES_API_TOKEN=<TOK> edgeone whoami   # auth OK (100016841661)

# trivial (functions + wasm work)
edgeone pages deploy deploy/eopages/trivial  --name gproxy-spike -t <TOK> -e production
#   -> https://gproxy-spike-te2iwbpy.edgeone.run?eo_token=<…>&eo_time=<…>
curl -L -c jar -b jar "https://gproxy-spike-te2iwbpy.edgeone.run/wasmtest?eo_token=<…>&eo_time=<…>"  # -> 5
curl -L      -b jar "https://gproxy-spike-te2iwbpy.edgeone.run/healthz"                              # -> hello-edgeone-pages

# real gproxy
cargo build --lib --target wasm32-unknown-unknown --release
bash deploy/eopages/build.sh                 # deno-target glue + base64-inline patch
edgeone pages deploy deploy/eopages/gproxy   --name gproxy-v2 -t <TOK> -e production
edgeone pages link                                 # (enter: gproxy-v2)  -> .edgeone/project.json
edgeone pages env set TURSO_URL   <REDACTED> -t <TOK>     # + TURSO_TOKEN, UPSTASH_URL, UPSTASH_TOKEN
edgeone pages deploy deploy/eopages/gproxy   --name gproxy-v2 -t <TOK> -e production
curl -L -c jar -b jar "https://gproxy-v2-g1yrgdxl.edgeone.run/healthz?eo_token=<…>&eo_time=<…>"
#   -> 404 from function (instantiate of 612KB wasm killed at ~15s; see probe table)
```

Verbatim failure signature (every wasm-instantiate path): no HTTP status, no
headers — `curl: (56) Recv failure: Connection reset by peer` at
`time_total ≈ 15.05–15.10 s`.

## Bottom line — Can gproxy's WASM run on EdgeOne Pages?

**WASM: YES. gproxy's wasm: NO (today).** EdgeOne Pages edge functions run
WebAssembly (proven live: `add(2,3) → 5`), and buffer-`instantiate` is allowed.
But gproxy's **612 KB** module exceeds the edge-function **wasm-instantiation
time budget (~15 s hard kill)**, so `/healthz` + `/version` never came up.

Paths forward to revisit (not done in this spike):
1. **Shrink the wasm** below the instantiation budget — `wasm-opt -Oz`, strip
   panic/format machinery, `opt-level="z"` + `lto`, drop unused deps — and
   re-test `/probeinst`. Today's module is 612 KB; the working probe was 41 B,
   so the threshold is somewhere between.
2. Try the **Pages Node Functions** runtime (full Node, longer compute budget)
   instead of Edge Functions — `node-functions/` + `WebAssembly.instantiate`
   from a `fs`-read `.wasm`. Different runtime; not an edge isolate.
3. File/confirm the exact Edge Function wasm-instantiation limit with Tencent.

## Reproduce

```bash
cd /home/linhuan/gproxy/v2
set -a && source ./.env && set +a            # EDGEONE_PAGES_API_TOKEN (+ GPROXY_TEST_* storage)
cargo build --lib --target wasm32-unknown-unknown --release
bash deploy/eopages/build.sh
# trivial wasm proof:
edgeone pages deploy deploy/eopages/trivial --name gproxy-spike -t "$EDGEONE_PAGES_API_TOKEN" -e production
#   then curl /wasmtest (carry the printed eo_token/eo_time + cookie jar) -> 5
# real gproxy (will reset at ~15s on /healthz):
edgeone pages deploy deploy/eopages/gproxy  --name gproxy-v2 -t "$EDGEONE_PAGES_API_TOKEN" -e production
```

**Cleanup:** three preview projects were created on the account —
`gproxy-spike` (`pages-duubpxy7tneq`, trivial + wasm probes),
`gproxy-v2` (`pages-zsrplszrfd5s`, real gproxy + storage env vars set),
`gproxy-iso` (`pages-aiopejin…`, throwaway isolation probes). They are
preview/preset deployments (token-gated, not public). No `teo` zone / DNS / paid
resource was provisioned. Delete from the EdgeOne Pages console if desired.
No curl was faked.
