# Tencent EdgeOne — WASM Edge Function Feasibility Spike

**Branch:** `phase-1i-edgeone-spike`
**Date:** 2026-06-03
**Question:** Can gproxy v2's WebAssembly edge build deploy + run on Tencent EdgeOne Edge Functions?

## STATUS: `FUNCTION_DEPLOY_BLOCKED`

Auth works and the Edge Function API is reachable (not allowlist-gated), but the
account has **no EdgeOne zone**, and creating one requires a real registered &
DNS-verified domain. With no zone, **no edge function — trivial or WASM — can be
deployed or served**. The WASM question therefore could NOT be answered
empirically with a live curl; see "Bottom line" for the documentation-based read.

## Tooling

- Direct Teo API calls via a self-contained TC3-HMAC-SHA256 signer:
  `deploy/edgeone/tc3_call.py` (reads creds from env; **no secrets stored**).
- Cross-checked with the official CLI **`tccli` version 3.1.104.1**
  (`pip install tccli`). Both clients produced identical results.
- Service: `teo`, Version: `2022-09-01`, Host: `teo.tencentcloudapi.com`.

## Step 1 — Auth + ZoneId resolution: AUTH OK, ZONE NOT FOUND

`DescribeZones {}` → `200`, `TotalCount: 0`, `Zones: []`. Auth succeeds (valid
RequestId, no SignatureFailure). Confirmed identically via
`tccli teo DescribeZones`.

- Filter by `zone-name=gproxy-eo` → `TotalCount: 0`. **`gproxy-eo` is NOT a real
  ZoneId and no such zone exists on the account.**
- `DescribeFunctions {"ZoneId":"gproxy-eo"}` →
  `ResourceUnavailable.ZoneNotFound` — "站点查询不到或不属于该账号"
  (site not found or does not belong to this account).

`DescribePlans {}` → `TotalCount: 1`: a **`plan-free`** plan
(`PlanId edgeone-3dieg1uzxe3s`, enabled 2025-06-30, `Status: normal`,
`Bindable: true`) but **`ZoneNumber: 0`, `ZonesInfo: []`** — the plan exists but
no zone is bound to it. Notably the plan's pay-as-you-go `Sv` resources DO
include edge-compute SKUs (`sv_edgeone_edge_computing_edgefunc_req_hour`,
`sv_edgeone_edge_computing_edgefunc_cputime_hour`), so Edge Functions are within
the plan's entitlement — they just have no zone to live in.

## Step 2 — Trivial JS function deploy: BLOCKED (no zone)

The Edge Function API exists and is usable on this account (NOT allowlist-gated):

- `CreateFunction {}` → `MissingParameter: ... required parameter 'Content'`
  (parameter-validation error, i.e. the API accepts the call).
- `CreateFunction` with a real trivial JS body
  (`addEventListener("fetch", e => e.respondWith(new Response("hello-edgeone")))`)
  **and** `ZoneId=gproxy-eo` → `ResourceUnavailable.ZoneNotFound`. The JS content
  was accepted; the **only** failure is the missing zone.

Creating a zone is the blocker and is out of scope for a trivial test:

- `CreateZone {}` → `InvalidParameterValue.ZoneNameInvalid` —
  "站点名称格式不正确，请输入正确的域名格式" (must be a valid **domain name**).
  A Teo zone requires a real registered domain plus DNS ownership verification;
  the spike has no authorized domain to bind, and provisioning one exceeds
  "create only a trivial test function".

EdgeOne **Pages** (the Git-integrated Functions/static product) has **no public
`teo` 2022-09-01 API**: `DescribePagesProjects` / `CreatePagesProject` /
`DescribePages` all → `InvalidAction` ("not found in service teo"). Pages is
console/Git-driven only, so it offers no API path to deploy a function here
either.

## Step 3 — WASM test: NOT REACHED

Cannot deploy any function without a zone, so no live WASM curl was possible.
A faked deploy/curl was explicitly avoided.

## Exact calls used (secrets redacted)

```
# Auth + zones (both signer and tccli)
DescribeZones {}                                            -> TotalCount 0
DescribeZones {"Filters":[{"Name":"zone-name","Values":["gproxy-eo"]}]} -> TotalCount 0
DescribePlans {}                                            -> 1x plan-free, ZonesInfo []
DescribeAvailablePlans {}                                   -> purchasable catalog (not owned)

# Function API reachability / blocker
DescribeFunctions {}                                        -> MissingParameter ZoneId
DescribeFunctions {"ZoneId":"gproxy-eo"}                    -> ResourceUnavailable.ZoneNotFound
CreateFunction {}                                           -> MissingParameter Content
CreateFunction {ZoneId:"gproxy-eo", Name, Content:<trivial js>} -> ResourceUnavailable.ZoneNotFound

# Zone creation requirement
CreateZone {}                                               -> InvalidParameterValue.ZoneNameInvalid (needs real domain)

# Pages surface (not on teo API)
DescribePagesProjects / DescribePages / CreatePagesProject  -> InvalidAction (not in service teo)

tccli teo DescribeZones   -> TotalCount 0   (independent confirmation)
tccli teo DescribePlans   -> 1x plan-free, ZonesInfo []
```

## Bottom line — Can gproxy's WASM run on Tencent EdgeOne?

**Not answered empirically — and not blocked on WASM itself, blocked on having a
zone.** To get any answer we'd first need to:
1. Register/own a domain and `CreateZone` it (DNS verification), then
2. `CreateFunction` + bind a `FunctionRule` route, then
3. test `WebAssembly.instantiate` inside the JS function and curl it.

Documentation read (not a substitute for the live test): EdgeOne markets Edge
Functions strictly as a **V8 + JavaScript / Web Service Worker** runtime. Its
published Runtime APIs (Fetch, Web Crypto, etc.) do **not** list WebAssembly, and
no `WebAssembly` global is documented. V8 technically supports WASM, but EdgeOne
neither advertises nor documents it, so it may be disabled/unsupported in the
isolate. **Treat WASM-on-EdgeOne as UNCONFIRMED / likely-unsupported** until a
function can actually be deployed (requires a domain-bound zone) and the
`WebAssembly.instantiate` curl returns `5`.

Refs: https://edgeone.ai/document/162227908259442688 ,
https://edgeone.ai/document/53374

## Reproduce

```bash
set -a && source ./.env && set +a    # TENCENTCLOUD_SECRET_ID / _KEY
python3 deploy/edgeone/tc3_call.py DescribeZones '{}'
python3 deploy/edgeone/tc3_call.py DescribePlans '{}'
python3 deploy/edgeone/tc3_call.py DescribeFunctions '{"ZoneId":"gproxy-eo"}'
# or: tccli teo DescribeZones
```

Cleanup: no resources were created (the only `CreateFunction` attempt failed with
ZoneNotFound), so nothing to tear down.
