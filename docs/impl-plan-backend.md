# gproxy v2 Backend — Implementation Spec (M1 detailed, M2–M10 contracts)

> Produced by the `v2-backend-contracts` workflow (5 parallel code/v1/doc readers →
> synthesis → 3 adversarial reviewers: compile-safety / architecture-fidelity /
> MVP-runnability → finalize). 27 issues found and resolved (3 blockers, 12 majors,
> 12 minors). This is the build bible for completing the v2 backend. The
> architecture rationale lives in `architecture-design.md` §3/§5/§6/§7; this doc is
> the compile-oriented implementation contract.

All signatures are grounded in the real code: `AppState` (`src/app.rs`),
`PersistenceBackend` (`src/store/persistence/mod.rs`), `CacheBackend`
(`src/store/cache/mod.rs`), `UpstreamClient` (`src/http/client/mod.rs:19`),
`OperationKey`/`Operation`/`ContentGenerationKind` (`src/protocol/operation.rs`),
the record `*Input` structs (`src/store/persistence/records/`). The dual
`cfg_attr async_trait` Send/?Send split is reproduced for every new trait so
wasm32 stays buildable.

Grounding confirmations: `wreq::Response::bytes_stream(self)` →
`impl Stream<Item = wreq::Result<Bytes>>` (needs `map_err`); `futures-util 0.3.32`
already transitive; `edge.rs:88` calls `AppState::new(...)` (needs patching);
cache trait has `incr`/`publish`/`subscribe`; no `blake3` dep yet; `dashmap`
already a dep; `User`/`UserKey` `*Input` fields confirmed (`is_admin`/`password`
on user, `label` on key); `Provider.channel: String`.

## Cross-cutting decisions (D1–D7)

- **D1 — One streaming Item error type, end to end.** `ByteStream` and
  `RespStream` are *the same typedef* `Item = Result<Bytes, ClientError>`.
  `ClientError: Error + Send + Sync + 'static`, so it satisfies axum
  `Body::from_stream`'s `S::Error: Into<BoxError>` with no conversion. No
  `io::Error` on the byte path; no re-box at the executor boundary.
- **D2 — `futures-util` is a direct dep (both targets).** `futures-core` only
  *names* `Stream`; constructing/adapting (`StreamExt`/`TryStreamExt::map_err`,
  `stream::once`) needs `futures-util`. Already transitive @0.3.32 → no version
  risk.
- **D3 — auth + classify run as pipeline steps; documented deviation from §4.**
  §4 lists auth/classify/ratelimit/permission as inbound middleware; M1 runs auth
  and classify as the first pipeline steps because classify is *provider-relative*
  (needs the parsed body) and auth must populate `RequestCtx.identity` before the
  scoped/aggregated branch. Auth still 401s **before any upstream candidate is
  built**. Logged in Risks; §4 line proposed for reconciliation.
- **D4 — One unified failover loop; streaming differs only at the body tail.**
  `run_failover` is the *only* place candidates are iterated and `classify` is
  called. Per candidate: one send (`send` buffered **or** `send_streaming`
  native) → uniform `(status, headers, BodySource)` → `classify` runs
  identically. Only terminal materialization differs:
  `BodySource::Buffered → ResponseBody::Full` vs
  `BodySource::Streaming → ResponseBody::Stream`. `pipeline/stream.rs` holds the
  tail helper only, not a parallel orchestrator.
- **D5 — Concrete path→OperationKey table ships in M1** (`pipeline/classify.rs`).
- **D6 — `GPROXY_SEED` is a real CLI flag; seeding + snapshot bootstrap fully
  specified in `main.rs` and `edge.rs`.**
- **D7 — `channel` → target `ContentGenerationKind` mapping pinned in M1** so the
  M2 `source_kind == target_kind` bypass predicate matches M1 passthrough.

## Post-review amendments (M1)

After M1 was implemented and adversarially reviewed, the following product
decisions superseded parts of §2 below:

- **No seeding.** The `--seed` / `GPROXY_SEED` flag and `src/seed.rs` are
  removed. There are **no built-in provider templates** either. All config
  (providers, routes, credentials, users/keys) arrives via config import (M9) or
  the admin API (M10); a fresh instance boots with an empty snapshot and serves
  nothing until config is loaded. (The §2 "HOW CONFIG IS SEEDED" subsection and
  the `scripts/smoke_m1.sh` it drove are obsolete; an e2e smoke returns once an
  import/admin bootstrap exists.)
- **Channel adapters are organized by AUTH mechanism, each self-managing.** The
  adapter boundary is the auth scheme, not the vendor: `openai_compatible`
  (Bearer api-key) serves all OpenAI-compatible vendors via `base_url` config;
  channels with distinct auth get their own adapter (`claude_api` = `x-api-key`;
  later `gemini_api`, and the OAuth/cookie/TLS channels each as their own,
  per `src/channel/registry.rs`). M1 ships `openai_compatible` + `claude_api`.
- **First-boot admin bootstrap (2026-06-10).** Deliberate exception to "no
  seeding" (which bans business-config seeds only): on startup, after
  persistence health and the §18 first-boot import (if any), if the `users`
  table is empty → create a default org (`default`) and user `admin`
  (`is_admin=true`) under it, with a CSPRNG-generated password (URL-safe, ≥24
  chars). Store the argon2id hash; print the plaintext ONCE to startup
  stdout/log in a prominent box with a change-it-now notice. Plaintext is never
  persisted or cached; never triggers on a non-empty store. Native-only (edge
  bootstrap follows its own import path). Companion override for password
  recovery: `--admin-user` / `GPROXY_ADMIN_USER` (default `admin`) +
  `--admin-password` / `GPROXY_ADMIN_PASSWORD`. When the password is set, every
  startup force-upserts that user at the bootstrap point — create as on first
  boot (given password, no random+print) or reset the hash and force
  `enabled=true` + `is_admin=true` (host-level recovery). Prominent warning on
  every startup while active; value never logged; env preferred over the CLI
  flag (cmdline is world-readable to other users on shared hosts). Same trust
  model as §18 import: host access already implies `GPROXY_MASTER_KEY` +
  data-dir/DSN access, so this adds no new attack surface. Lands with the
  admin API (M9).

---

## 1. Shared contracts

### 1.1 `Channel` trait — `src/channel/mod.rs`

One `async_trait` block (only `refresh` is async; the macro leaves sync defaults
untouched). **Purity:** `prepare` does *access* only — auth injection,
endpoint/path/base_url composition. `upstream_model_id` is used for **path
construction only** (path-templated providers like Gemini), never to mutate the
body. M1 same-protocol forwards the body's `model` **verbatim**; body model
rewrite is a process/transform concern (M2, §3.4/§6.1).

```rust
use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use serde_json::Value;
use std::sync::Arc;

use crate::channel::prepared::PreparedRequest;
use crate::channel::disposition::Disposition;
use crate::http::client::{ClientError, UpstreamClient};
use crate::store::persistence::records::Credential;
use crate::protocol::ContentGenerationKind;

/// Declared transport for capability-based degradation (§7.4). M1: Http only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind { Http, Ws }

/// Per-call inputs to build the upstream request. `'a` borrows snapshot-owned
/// data; `body` is owned (Bytes) and is MOVED into the request by `prepare`.
pub struct PrepareCtx<'a> {
    pub secret: &'a Value,            // Credential.secret_json (decrypted at use)
    pub provider_settings: &'a Value, // Provider.settings_json (base_url, toggles)
    pub upstream_model_id: &'a str,   // PATH construction only; never body mutation
    pub method: http::Method,
    pub path: &'a str,                // inbound provider-relative path (post-strip in scoped)
    pub query: Option<&'a str>,
    pub headers: &'a HeaderMap,       // sanitized inbound headers
    pub body: Bytes,                  // forwarded verbatim (same-protocol)
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait Channel: Send + Sync {
    /// Registry key (matches Provider.channel).
    fn id(&self) -> &'static str;

    /// Native content-generation wire format. Pins the M2 transform-bypass
    /// predicate at M1 time. openai_compatible -> OpenAiChatCompletions;
    /// claude_api -> ClaudeMessages.
    fn target_kind(&self) -> ContentGenerationKind;

    /// Inject auth, resolve endpoint+method, set ABSOLUTE upstream URL. Pure
    /// access; NO transform/rules, NO body mutation. Moves `ctx.body` in.
    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError>;

    /// Map (status, headers, body-preview) to the 5-state Disposition. For
    /// streaming, `body` is the first-frame preview (status+headers suffice).
    fn classify(&self, status: StatusCode, headers: &HeaderMap, body: &Bytes) -> Disposition;

    /// Channel-specific fixups before transform. M1 same-protocol: identity.
    fn normalize(&self, body: Bytes) -> Bytes { body }

    fn needs_refresh(&self, _cred: &Credential) -> bool { false }

    async fn refresh(
        &self,
        _client: &Arc<dyn UpstreamClient>,
        _cred: &Credential,
    ) -> Result<Value, ChannelError> {
        Err(ChannelError::Unsupported("refresh"))
    }

    fn transport(&self) -> TransportKind { TransportKind::Http }
    fn requires_tls_emulation(&self) -> bool { false }
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("missing setting: {0}")] MissingSetting(&'static str),
    #[error("invalid credential: {0}")] InvalidCredential(String),
    #[error("unsupported: {0}")] Unsupported(&'static str),
    #[error("build error: {0}")] Build(String),
}
```

`Arc<dyn Channel>` stays `Send + Sync` on both targets (supertrait constrains the
impl type; `?Send` only affects returned futures on wasm).

### 1.2 `PreparedRequest` — `src/channel/prepared.rs`

```rust
use bytes::Bytes;

pub struct PreparedRequest {
    /// MUST be absolute (scheme+authority+path+query) — wreq cannot route a
    /// relative URI. See http_util::build_request (§1.10).
    pub request: http::Request<Bytes>,
    /// Per-attempt proxy override (Credential.proxy_url ?? provider default).
    /// Native only; ignored in M1. Carried for the §7.4 client pool key.
    pub proxy_url: Option<String>,
}

impl PreparedRequest {
    pub fn into_http(self) -> http::Request<Bytes> { self.request }
}
```

> **M1 transport reality:** per-credential proxy is realized in M4/M7 via
> `WreqClient`'s internal `(proxy_url, tls_emulation)`-keyed client pool (§7.4).
> `UpstreamClient::send` stays as-is; there is **no** `send_with_proxy` method.

### 1.3 `Disposition` — `src/channel/disposition.rs`

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    Success,                                              // 2xx
    AuthDead,                                             // 401/402/403
    RateLimited { retry_after: Option<std::time::Duration> }, // 429
    Transient,                                            // 5xx / network
    Permanent,                                            // 4xx validation
}

impl Disposition {
    pub fn is_success(&self) -> bool { matches!(self, Self::Success) }
    pub fn should_failover(&self) -> bool {
        matches!(self, Self::AuthDead | Self::RateLimited { .. } | Self::Transient)
    }
}
```

### 1.4 `ExecOutcome` + body — `src/pipeline/outcome.rs`

Byte-stream Item error is `ClientError` end to end (D1); identical to `RespStream`
(§1.9). `Stream` variant is `Send`-bounded native, cfg-split on wasm (wasm M1 = `Full` only).

```rust
use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use crate::channel::disposition::Disposition;
use crate::http::client::ClientError;

#[cfg(not(target_arch = "wasm32"))]
pub type ByteStream =
    std::pin::Pin<Box<dyn futures_core::Stream<Item = Result<Bytes, ClientError>> + Send>>;
#[cfg(target_arch = "wasm32")]
pub type ByteStream =
    std::pin::Pin<Box<dyn futures_core::Stream<Item = Result<Bytes, ClientError>>>>;

pub struct ExecOutcome {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: ResponseBody,
    pub disposition: Disposition,
}

pub enum ResponseBody {
    Full(Bytes),
    #[cfg(not(target_arch = "wasm32"))]
    Stream(ByteStream),
}

impl ResponseBody {
    pub fn full(b: impl Into<Bytes>) -> Self { Self::Full(b.into()) }
}
```

> **Guardrail (D1):** keep the byte-stream Item error a concrete
> `Error + Send + Sync + 'static` (here `ClientError`). Never "simplify" to
> `String` or a non-Send type — `Body::from_stream` would reject it.

### 1.5 `ControlPlaneSnapshot` — `src/app/snapshot.rs`

The **sole** control-plane ArcSwap snapshot (§7.2). M2/M3 extend THIS struct +
`build()`, never a parallel one. Rebuildable from persistence (boot + invalidation).

```rust
use std::collections::HashMap;
use std::sync::Arc;
use crate::store::persistence::PersistenceBackend;
use crate::store::persistence::records::{
    Alias, Credential, Provider, ProviderModel, Route, RouteMember, User, UserKey,
};

pub struct ControlPlaneSnapshot {
    pub providers_by_name: HashMap<String, Arc<Provider>>,
    pub providers_by_id: HashMap<i64, Arc<Provider>>,
    pub routes_by_name: HashMap<String, Arc<ResolvedRoute>>,
    pub alias_to_route: HashMap<String, String>,
    pub keys_by_digest: HashMap<String, Arc<KeyIdentity>>, // ENABLED keys + users only
    pub credentials_by_provider: HashMap<i64, Vec<Arc<Credential>>>, // ENABLED only
    pub models_by_provider: HashMap<i64, Vec<Arc<ProviderModel>>>,
    pub version: u64,
}

/// Route + members pre-sorted by (tier asc, weight desc).
pub struct ResolvedRoute { pub route: Route, pub members: Vec<RouteMember> }

/// Auth identity resolved from a user key (org_id/team_id used by M3).
pub struct KeyIdentity { pub user_key: UserKey, pub user: User }

impl ControlPlaneSnapshot {
    /// Full reload (boot + invalidation). On wasm the backend is `?Send`, so
    /// this future is non-Send; never put it on a Send-requiring spawn there.
    pub async fn build(db: &dyn PersistenceBackend, version: u64) -> anyhow::Result<Self> {
        // 1. list_providers -> by_{name,id}; per provider list_credentials(enabled)
        //    + list_provider_models.
        // 2. list_routes; per route list_route_members; sort (tier asc, weight desc).
        // 3. list_aliases; alias.route_id -> route.name -> alias_to_route.
        // 4. list_users(enabled); per user list_user_keys(enabled) ->
        //    keys_by_digest[api_key_digest] = KeyIdentity{user_key, user}.
        unimplemented!()
    }
    pub fn empty(version: u64) -> Self { unimplemented!() }
}
```

> M1 builds `keys_by_digest` (pure snapshot read, no DB hit on hot path) rather
> than calling `find_user_key_by_digest`. Seed and auth must agree on the digest
> input (the bare token — §1.8).

### 1.6 `AppState` — `src/app.rs` → `src/app/mod.rs` (modify)

`app.rs` becomes `app/mod.rs` so `snapshot.rs` is a submodule; `pub use` keeps
`crate::app::AppState` stable for `main.rs`, `http/server`, `http/edge.rs`.

```rust
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RuntimeConfig>,
    pub cache: Arc<dyn CacheBackend>,
    pub persistence: Arc<dyn PersistenceBackend>,
    pub upstream: Arc<dyn UpstreamClient>,
    pub snapshot: Arc<arc_swap::ArcSwap<ControlPlaneSnapshot>>, // new
    pub channels: Arc<ChannelRegistry>,                          // new
}

impl AppState {
    pub fn new(/* 6 args */) -> Self { /* ... */ }
    pub fn cp(&self) -> arc_swap::Guard<Arc<ControlPlaneSnapshot>> { self.snapshot.load() }
    pub async fn reload_snapshot(&self) -> anyhow::Result<()> {
        let next = self.cp().version.wrapping_add(1);
        let snap = ControlPlaneSnapshot::build(self.persistence.as_ref(), next).await?;
        self.snapshot.store(Arc::new(snap));
        Ok(())
    }
}
```

### 1.7 `ChannelRegistry` — `src/channel/registry.rs`

```rust
pub struct ChannelRegistry { map: HashMap<&'static str, Arc<dyn Channel>> }

impl ChannelRegistry {
    /// Builds the M1 channel set. Wasm-clean (http + serde_json only).
    pub fn with_builtin() -> Self {
        let mut map: HashMap<&'static str, Arc<dyn Channel>> = HashMap::new();
        let oc: Arc<dyn Channel> = Arc::new(crate::channel::openai_compatible::OpenAiCompatChannel);
        map.insert(oc.id(), oc);
        let cc: Arc<dyn Channel> = Arc::new(crate::channel::claude_api::ClaudeApiChannel);
        map.insert(cc.id(), cc);
        Self { map }
    }
    pub fn get(&self, id: &str) -> Option<Arc<dyn Channel>> { self.map.get(id).cloned() }
}
```

### 1.8 Pipeline step signatures — `src/pipeline/`

Free functions threaded by a per-request `RequestCtx`. Auth + classify are the
first two steps (D3). `run_failover` is the only candidate-iteration/classify site (D4).

```rust
// pipeline/context.rs
pub struct RequestCtx {
    pub request_id: String,
    pub method: http::Method,
    pub path: String,         // provider-relative (/v1/...); scoped already stripped
    pub query: Option<String>,
    pub headers: http::HeaderMap,
    pub body: bytes::Bytes,
    pub mode: RoutingMode,
    pub identity: Option<std::sync::Arc<crate::app::snapshot::KeyIdentity>>,
    pub op: Option<crate::protocol::OperationKey>,
    pub stream: bool,
    pub route_name: Option<String>,
}
pub enum RoutingMode { Aggregated, Scoped { provider: String } }
pub struct Candidate {
    pub provider: std::sync::Arc<crate::store::persistence::records::Provider>,
    pub credential: std::sync::Arc<crate::store::persistence::records::Credential>,
    pub upstream_model_id: String,
}
pub struct Classified { pub op: crate::protocol::OperationKey, pub stream: bool }

// pipeline/error.rs  — IntoResponse with these statuses:
//   Unauthorized->401, UnknownRoute/UnknownProvider/UnsupportedPath->404,
//   NoMembers/NoCredentials->503, UnknownChannel->500, Channel->502,
//   AllAttemptsFailed->502, Transport->502.

// pipeline/auth.rs
pub fn extract_bearer(headers: &http::HeaderMap) -> Option<String>; // Bearer then x-api-key, bare token
pub fn authenticate(cp: &ControlPlaneSnapshot, headers: &http::HeaderMap)
    -> Result<Arc<KeyIdentity>, PipelineError>;  // 401 short-circuit BEFORE any candidate
pub fn key_digest(bare_token: &str) -> String;   // hex(blake3(token)); SINGLE source of truth (seed + auth)

// pipeline/classify.rs — hardcoded (method,path)->OperationKey table (D5)
pub fn classify(method: &http::Method, path: &str, body: &bytes::Bytes)
    -> Result<Classified, PipelineError>;

// pipeline/preprocess.rs — AGGREGATED only: model -> alias_to_route -> route name
pub fn preprocess(cp: &ControlPlaneSnapshot, ctx: &RequestCtx) -> Result<String, PipelineError>;

// pipeline/route.rs
pub fn route<'a>(cp: &'a ControlPlaneSnapshot, route_name: &str)
    -> Result<&'a Arc<ResolvedRoute>, PipelineError>;

// pipeline/balance.rs — M1: tier-0 members in order, round-robin credential
pub fn candidates(cp: &ControlPlaneSnapshot, route: &ResolvedRoute,
    cache: &dyn CacheBackend, affinity_key: Option<&str>)
    -> Result<Vec<Candidate>, PipelineError>;

// pipeline/execute.rs — THE generic orchestrator (aggregated + scoped)
pub async fn execute(state: &AppState, ctx: RequestCtx)
    -> Result<ExecOutcome, PipelineError>;

// pipeline/failover.rs — the ONE 5-state loop + classify site (D4)
pub async fn run_failover(state: &AppState, ctx: &RequestCtx,
    channel: &Arc<dyn Channel>, candidates: &[Candidate])
    -> Result<ExecOutcome, PipelineError>;

pub enum BodySource {
    Buffered(bytes::Bytes),
    #[cfg(not(target_arch = "wasm32"))]
    Streaming(crate::http::client::RespStream),
}
```

**M1 classification table** (match `(method, path)`; `/v1` prefix present in both
modes after scoped strip):

| Method | Path | OperationKey | `stream` source |
|---|---|---|---|
| POST | `/v1/chat/completions` | `content_generation(GenerateContent, OpenAiChatCompletions)` | body `stream` |
| POST | `/v1/responses` | `content_generation(GenerateContent, OpenAiResponses)` | body `stream` |
| POST | `/v1/messages` | `content_generation(GenerateContent, ClaudeMessages)` | body `stream` |
| POST | `/v1/embeddings` | `provider(CreateEmbedding, OpenAi)` | false |
| POST | `/v1/images/generations` | `provider(CreateImage, OpenAi)` | false |
| GET  | `/v1/models` | `provider(ListModels, OpenAi)` | false |
| POST | `…/models/{model}:generateContent` | `content_generation(GenerateContent, GeminiGenerateContent)` | false |
| POST | `…/models/{model}:streamGenerateContent` | `content_generation(StreamGenerateContent, GeminiGenerateContent)` | true |

Gemini rows match on the `:generateContent`/`:streamGenerateContent` path suffix
(after the last `/`), independent of `{model}`. Smoke test exercises row 1 only.

> **Scoped branch in `execute.rs`** (`mode == Scoped { provider }`): auth +
> classify as usual; **skip** preprocess/route; `providers_by_name.get(provider)`
> → `UnknownProvider`; synthesize candidates from
> `credentials_by_provider[provider.id]` (RR), `upstream_model_id = body model`
> verbatim → `NoCredentials` if empty; model validation lax in M1 (Risk 9);
> leading `/{provider}` already stripped in `extract.rs` so `path` is `/v1/...`.

`pipeline/stream.rs` (native) holds only the tail helper:

```rust
#[cfg(not(target_arch = "wasm32"))]
pub fn into_byte_stream(s: crate::http::client::RespStream)
    -> crate::pipeline::outcome::ByteStream { s } // identity (same typedef, D1)
```

### 1.9 `UpstreamClient` streaming — `src/http/client/mod.rs` (modify)

Default method so existing impls + wasm compile unchanged. `RespStream` ==
`ByteStream` (D1). Default + wreq override use `futures-util` (D2).

```rust
#[cfg(not(target_arch = "wasm32"))]
pub type RespStream =
    std::pin::Pin<Box<dyn futures_core::Stream<Item = Result<Bytes, ClientError>> + Send>>;

// in trait UpstreamClient:
#[cfg(not(target_arch = "wasm32"))]
async fn send_streaming(&self, req: http::Request<Bytes>)
    -> Result<(StatusCode, HeaderMap, RespStream), ClientError>
{
    use futures_util::StreamExt;
    let resp = self.send(req).await?;
    let (parts, body) = resp.into_parts();
    let once = futures_util::stream::once(async move { Ok::<Bytes, ClientError>(body) });
    Ok((parts.status, parts.headers, once.boxed()))
}
```

`wreq.rs` override:

```rust
#[cfg(not(target_arch = "wasm32"))]
async fn send_streaming(&self, req: http::Request<Bytes>)
    -> Result<(StatusCode, HeaderMap, RespStream), ClientError>
{
    use futures_util::{StreamExt, TryStreamExt};
    let wreq_req: wreq::Request = req.into();
    let resp = self.inner.execute(wreq_req).await
        .map_err(|e| ClientError::Transport(e.to_string()))?;
    let status = resp.status();
    let headers = resp.headers().clone();
    let stream: RespStream = resp.bytes_stream()
        .map_err(|e| ClientError::Transport(e.to_string()))
        .boxed();
    Ok((status, headers, stream))
}
```

### 1.10 `http_util` — `src/channel/http_util.rs`

```rust
/// Absolute URI from base_url + provider-relative path + query. Trims one
/// trailing '/' off base; MissingSetting("base_url") if absent; Build if result
/// is not absolute (no scheme/authority). Debug-asserts absolute.
pub fn join_url(base_url: &str, path: &str, query: Option<&str>) -> Result<http::Uri, ChannelError>;

/// Hop-by-hop headers stripped on ingress AND egress: transfer-encoding,
/// content-length, connection, keep-alive, upgrade, proxy-authenticate,
/// proxy-authorization, te, trailer. Also strips Host (a fresh Host is set).
pub const HOP_BY_HOP: &[http::HeaderName];
pub fn sanitize_headers(src: &http::HeaderMap) -> http::HeaderMap;

/// Final upstream request: method + absolute URI + sanitized headers + Host from
/// authority; moves body in. Channel auth headers inserted by the caller AFTER.
pub fn build_request(method: http::Method, uri: http::Uri, headers: http::HeaderMap,
    body: bytes::Bytes) -> Result<http::Request<bytes::Bytes>, ChannelError>;
```

- `OpenAiCompatChannel::prepare`: `join_url` + `build_request`, insert
  `Authorization: Bearer <secret["api_key"]>`. `id="openai_compatible"`,
  `target_kind=OpenAiChatCompletions`.
- `ClaudeApiChannel::prepare`: same join/build, insert `x-api-key` +
  `anthropic-version: 2023-06-01`. `id="claude_api"`, `target_kind=ClaudeMessages`.

---

## 2. M1 file plan

Legend: **C** create, **M** modify. Lines are budget targets (hard cap 500/file).

### app / state
| Path | Resp. | Lines |
|---|---|---|
| **M** `src/app.rs` → `src/app/mod.rs` | dir-module; add `snapshot`/`channels` + 6-arg `new`, `cp()`, `reload_snapshot()` | +45 |
| **C** `src/app/snapshot.rs` | snapshot types + `build()`(enabled-only) + `empty()` | ~160 |

### channel/
| Path | Resp. | Lines |
|---|---|---|
| **C** `src/channel/mod.rs` | `Channel` (+`target_kind`), `PrepareCtx`, `TransportKind`, `ChannelError`, submods | ~130 |
| **C** `src/channel/disposition.rs` | `Disposition` + helpers | ~50 |
| **C** `src/channel/prepared.rs` | `PreparedRequest` | ~30 |
| **C** `src/channel/registry.rs` | `ChannelRegistry` (wasm-clean) | ~45 |
| **C** `src/channel/http_util.rs` | `join_url`/`sanitize_headers`/`HOP_BY_HOP`/`build_request` (+abs-URI test) | ~120 |
| **C** `src/channel/openai_compatible.rs` | OpenAI-compatible channel | ~110 |
| **C** `src/channel/claude_api.rs` | Claude api-key channel | ~110 |

### pipeline/
| Path | Resp. | Lines |
|---|---|---|
| **C** `src/pipeline/mod.rs` | re-exports / `pub mod` | ~25 |
| **C** `src/pipeline/context.rs` | `RequestCtx`/`RoutingMode`/`Candidate`/`Classified` | ~75 |
| **C** `src/pipeline/error.rs` | `PipelineError` + `IntoResponse` | ~80 |
| **C** `src/pipeline/auth.rs` | `extract_bearer`/`authenticate`/`key_digest` | ~70 |
| **C** `src/pipeline/classify.rs` | path→OperationKey table + stream peek | ~170 |
| **C** `src/pipeline/preprocess.rs` | model→alias→route name | ~80 |
| **C** `src/pipeline/route.rs` | route lookup | ~30 |
| **C** `src/pipeline/balance.rs` | candidate list | ~110 |
| **C** `src/pipeline/execute.rs` | generic orchestrator; aggregated + scoped | ~150 |
| **C** `src/pipeline/failover.rs` | the ONE 5-state loop; `BodySource` unify; tail | ~170 |
| **C** `src/pipeline/stream.rs` (native) | tail-only `into_byte_stream` | ~40 |
| **C** `src/pipeline/outcome.rs` | `ExecOutcome`/`ResponseBody`/`ByteStream` | ~60 |

### http/server + client
| Path | Resp. | Lines |
|---|---|---|
| **M** `src/http/server/mod.rs` | register gateway routes; body-limit | +25 |
| **C** `src/http/server/gateway.rs` | handlers; build `RequestCtx`; call execute; map `ExecOutcome`→response | ~150 |
| **C** `src/http/server/extract.rs` | body→Bytes; `RoutingMode` + strip `/{provider}`; request-id | ~90 |
| **M** `src/http/client/mod.rs` | `RespStream` + `send_streaming` default | +45 |
| **M** `src/http/client/wreq.rs` | override `send_streaming` | +35 |

### edge / seed / wiring
| Path | Resp. | Lines |
|---|---|---|
| **M** `src/http/edge.rs` | build snapshot + registry; 6-arg `AppState::new` | +20 |
| **C** `src/seed.rs` | `seed_if_empty(db)`: emptiness-gated upserts | ~160 |
| **M** `src/main.rs` | `--seed`/`GPROXY_SEED`; seed; build snapshot+registry; 6-arg new | +30 |
| **M** `src/lib.rs` | `pub mod channel; pub mod pipeline; pub mod seed;` | +3 |
| **M** `Cargo.toml` | `futures-core`/`futures-util`/`blake3` (both), `uuid` (native) | — |

`src/http/server/admin.rs` deferred to M9 (seed covers M1).

**Cargo deps:** `futures-core="0.3"`, `futures-util="0.3"`, `blake3="1"` in
`[dependencies]`; `uuid={version="1",features=["v4"]}` native-only (avoids wasm
`getrandom/js`; wasm request-id uses `js_sys::Date::now()` + counter).

### Seeding (programmatic, backend-agnostic, gated by `GPROXY_SEED` + emptiness)

`seed_if_empty(db)`: if `!list_providers().is_empty()` return. Else upsert in FK
order: `org("default")` → `team(org,"default")` →
`provider("mock-openai", channel="openai_compatible", settings_json={"base_url":"http://127.0.0.1:9009"}, credential_strategy="round_robin")`
→ `credential(provider, kind="api_key", secret_json={"api_key":"sk-mock"})` →
`provider_model(provider,"gpt-4o-mini")` → `route("gpt-4o-mini", strategy="weighted")`
→ `route_member(route, provider, upstream_model_id="gpt-4o-mini", tier=0, weight=1)`
→ `alias("gpt-4o-mini", route)` →
`user("smoke", org, team, is_admin=false, password=None)` →
`user_key(user, api_key_digest=key_digest("sk-smoke-123"), label=None)`.

`main.rs`: add `#[arg(long, env="GPROXY_SEED", default_value_t=false)] seed`;
after `persistence.health()` and before `AppState::new`, if `cli.seed` call
`seed::seed_if_empty`; then `ControlPlaneSnapshot::build(persistence, 1)` →
`ArcSwap::from_pointee` → `ChannelRegistry::with_builtin()` → 6-arg `AppState::new`.
`edge.rs:88`: build snapshot over libsql + registry (no seed on edge).

---

## 3. Integration order (both targets green each step)

Each step keeps `cargo build` (default) AND
`cargo build --target wasm32-unknown-unknown --features edge` green. `Stream`
body, `pipeline/stream.rs`, `send_streaming`, `BodySource::Streaming` are all
`#[cfg(not(wasm32))]`.

1. Cargo deps (no code).
2. `channel/disposition.rs` + `channel/prepared.rs` (leaf types).
3. `channel/mod.rs`.
4. `pipeline/outcome.rs`.
5. `channel/http_util.rs` + `openai_compatible.rs` + `claude_api.rs` + `registry.rs`.
6. `app/snapshot.rs`.
7. `app.rs`→`app/mod.rs`; 6-arg `new`; **patch all three call sites now** (main/edge
   temporarily pass `empty(0)` snapshot + registry).
8. `pipeline/context.rs` + `error.rs`.
9. `pipeline/auth.rs`, `classify.rs`, `preprocess.rs`, `route.rs`, `balance.rs`.
10. `http/client` streaming (`mod.rs` default + `wreq.rs` override).
11. `pipeline/stream.rs`.
12. `pipeline/failover.rs` then `execute.rs`.
13. `http/server/extract.rs` + `gateway.rs`; register routes in `server/mod.rs`.
14. `seed.rs` + `main.rs` wiring; replace temp empty snapshot with real `build`;
    update `edge.rs` to real `build`.

### Router (axum 0.8) — `src/http/server/mod.rs`

```rust
Router::new()
    .route("/healthz", get(health::healthz))
    .route("/version", get(health::version))
    .route("/v1/{*rest}", any(gateway::aggregated))        // registered FIRST (literal /v1 wins)
    .route("/{provider}/v1/{*rest}", any(gateway::scoped)) // scoped; rejects provider=="v1"
    .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
    .with_state(state)
```

- Disambiguation: `/v1/...` before `/{provider}/v1/...`; `scoped` rejects
  `provider=="v1"` defensively. "v1" reserved as non-provider segment.
- Request-id generated in `extract.rs` (native `Uuid::new_v4()`, wasm
  `Date::now()`+counter). M1 middleware = only `DefaultBodyLimit::max(16 MiB)`.

### Egress — `src/http/server/gateway.rs`

read body→Bytes + `RoutingMode` + request-id + sanitize headers → `execute` →
build `Response` with `outcome.status`, copy headers minus `HOP_BY_HOP`, body:
`Full(b)→Body::from(b)`; `Stream(s)→Body::from_stream(s)` (s Item =
`Result<Bytes,ClientError>`, `ClientError: Into<BoxError>+Send+'static`, D1).
`extract.rs` strips leading `/{provider}` for scoped so `path` is `/v1/...` in
both modes; dropping `content-length`/`transfer-encoding` lets the client read
the close-delimited SSE body.

---

## 4. Smoke test (executable, against a mock upstream)

Boot file backend with `GPROXY_SEED=1`; mock upstream on :9009 logging hits and
asserting `Authorization == Bearer sk-mock` (proves channel injected the
credential, not the inbound key). Assertions:

1. **Non-stream aggregated** `POST /v1/chat/completions` model `gpt-4o-mini` →
   body contains `"content":"pong"`.
2. **Stream passthrough** `stream:true` → exactly **2** `data:` frames forwarded.
3. **Auth fail** bad key → **401** AND **zero** upstream hits (D3 short-circuit).
4. **Scoped bypass** `POST /mock-openai/v1/chat/completions` → `"content":"pong"`.

(Full mock + curl script in the workflow output; reproduce under
`scripts/smoke_m1.sh` when implementing.)

---

## 5. M2–M10 interface contracts

Each lists new types/methods + the exact M1 seam they hook into.

- **M2 Transform + Process.** `pipeline/transform.rs` over `transform::resolve`;
  `process::Processor::apply`. **Model rewrite lives here**, not in the channel.
  Bypass predicate `source_kind == target_kind` (`source = ctx.op.kind`,
  `target = channel.target_kind()`, D7) in `execute.rs` between balance and
  failover. Per-frame `dispatch_stream_event` splices at
  `pipeline/stream.rs::into_byte_stream` — failover loop unchanged (D4). Snapshot
  gains `rule_sets_by_provider`; `ResolvedRoute` gains per-member target kind.
  **Landed (2026-06-10).** Deviations from the original contract: (1) the
  bypass predicate + plan run PER CANDIDATE in the failover loop head (members
  span channels with different native kinds; the channel — hence
  `target_kind()` — is resolved there), not between balance and failover;
  (2) `ResolvedRoute` did NOT gain per-member target kind (redundant with the
  per-candidate channel); (3) `process::apply` is a free function;
  (4) non-content pair dispatch (count_tokens/models/embeddings/images/
  compact) and §6.2 signature compat are deferred — `dispatch::is_wired`
  gates them; (5) non-2xx upstream bodies return provider-native (error-shape
  conversion is a fidelity follow-up); (6) routing `local` returns 501 until
  the in-memory models implementation lands with classify coverage for models
  operations. A minimal bundle import (M9-lite: `gproxy import --in` +
  `GPROXY_IMPORT_FILE` first-boot hook) landed as the e2e enabler; the FILE
  backend's `upsert_*` now insert at explicit ids (advancing `next_id`); the
  DB backend still errors on explicit-id-not-found — **M9 must port the same
  upsert semantics to the DB backend before `import --persistence=db` works.**
  **M2.5 landed (2026-06-10): transform surface completion.** classify covers
  models/count_tokens paths (`GET /v1/models` openai/claude collision resolved
  by credential form: `x-api-key` → claude); `request_target` synthesizes all
  wired operation endpoints (compact = `/v1/responses/compact` per v1 sdk);
  bytes dispatch covers all 37 pairs (gemini batch embeddings deferred —
  single `:embedContent` form only); non-content Provider-kind transform
  targets flow through request_parts (memo keyed by OperationKind). `local`
  is real: aggregated `/v1/models` lists alias+route names; scoped models
  local/merged lists with `provider_models.variants_json` suffix variants
  (request-side strip via snapshot variant index); count_tokens served by
  `src/tokenize/` — tiktoken (gpt heuristic) / bundled deepseek-v4-pro vocab
  (6.4 MB, `assets/tokenizers/`) / HF download via TokenizerRegistry
  (vocabs stored through PersistenceBackend: file = raw files, db =
  `tokenizer_vocabs` BLOBs; gate `instance_settings.enable_tokenizer_download`,
  default off) / chars/2 estimate floor (edge). Default routing: CountTokens
  on openai-family channels → Local (explicit rule opts into passthrough);
  count falls back to local when every upstream candidate fails. Process
  `filter_model_pattern` now matches the inbound pre-variant-strip model name.
  Known gaps: inbound compact classify deferred; images/compact cross-op
  reachable only via explicit routing rules; db backends created before this
  milestone lack the two new columns (schema-from-entity, no migrations yet —
  M9 concern).
- **M3 Authz 3-level.** `pipeline/authz.rs`: `check_permission` (union
  user/team/org), `precheck_limits` (strictest of 3, via `cache.incr`). Inserted
  after `route()` before `balance()`. Counters redis-direct, not snapshot.
  Snapshot gains permissions/limits/quotas + orgs/teams. (§4 reconciliation, D3.)
- **M4 Balance + breaker + cooldown.** `balance/strategy.rs`; local soft
  `CredentialHealth` as a 2nd `AppState` field (DashMap). `candidates` filled with
  real strategy + filters; `failover` calls `health.record_*` on each
  Disposition (branch points exist in the single loop). Per-credential proxy via
  WreqClient `(proxy_url, tls_emulation)` pool (`PreparedRequest.proxy_url` key).
- **M5 Security envelope.** `crypto/envelope.rs` `{kek_id, wrapped_dek, nonce,
  ciphertext}`; decrypt `credential.secret_json`→plaintext `Value` before
  `PrepareCtx.secret`. No signature change (`secret: &Value` already "decrypted at
  use"). Seed switches to envelope form. `key_digest` salt/pepper revisited.
- **M6 Billing + observability.** `billing/record.rs`: `record_success`
  (idempotent by `request_id`) via `append_usage` + `add_usage_rollup`;
  `record_failure` per failed attempt via `append_upstream_request`. The single
  failover loop already isolates final-Success vs failed-attempt.
- **M7 Resilience.** Real `Channel::refresh`/`needs_refresh`; retry budgets. The
  `AuthDead` branch already calls `refresh` once then retries — M7 fills the body.
  `requires_tls_emulation`/`transport` drive §7.4 degradation.
- **M8 Multi-instance redis.** Real `CacheBackend::publish`/`subscribe`;
  `app/invalidation.rs` subscribe → `reload_snapshot()`. `version`+`ArcSwap` are
  the consistency primitive; no snapshot shape change. (Wasm: `reload_snapshot`
  future is non-Send; never spawn.)
- **M9 Export/Import.** `admin/export.rs`/`import.rs`: serialize all records to a
  bundle; import via `upsert_*` (reuses the seed pattern; `*Input` = import
  schema). `admin.rs` deferred from M1 lands here. First-boot admin bootstrap
  lands here too, ordered AFTER the first-boot import (see post-review
  amendments).
  Note: M2 landed an import enabler (see M2 bullet); port the file backend's
  explicit-id upsert semantics to the DB backend here.
- **M10 (reserved/hardening).** Rate-limit fairness, quota reset jobs, admin
  console; over existing `cache.incr` + snapshot configs.

---

## 6. Risks / open decisions

1. **Streaming extends `UpstreamClient`** via a native-only `send_streaming`
   default (buffer-and-wrap); `WreqClient` overrides via `bytes_stream()`. Item
   error `ClientError` end-to-end (D1).
2. **Stream typing unified (D1) + `futures-util` direct dep (D2).** Guardrail:
   never change the stream Item error to a non-Error/non-Send type.
3. **wasm streaming blocked** (fetch reads whole body; `?Send`). M1 stream paths
   are all `#[cfg(not(wasm32))]`; edge serves streaming requests buffered.
4. **Per-credential proxy not honored in M1** (`WreqClient` built once). Field
   carried; realized in M4/M7 via the client pool.
5. **Digest scheme:** `key_digest = hex(blake3(bare_token))`; single source of
   truth (seed + auth). Salt/pepper deferred to M5.
6. **Seed FK order resolved:** prepend org+team; all `*Input` fields filled;
   works on file AND db.
7. **`app.rs`→`app/mod.rs`:** `pub use` keeps `crate::app::AppState` stable; all
   three `new` call sites patched (main/server/edge); both targets green per step.
8. **Classify body-peek:** minimal targeted parse (`{model, stream}` only), not a
   full protocol deserialize.
9. **Scoped model validation lax in M1** (provider must exist; model forwarded
   verbatim). Strict check in M3.
10. **Deliberate §4 deviation (D3):** auth/classify (M1) + ratelimit/permission
    (M3) run as pipeline steps, not middleware; externally-observable semantics
    preserved (401 before upstream). **Action:** update §4's middleware line.
11. **`instance_id` (u64) vs §8-E `instance_name` drift.** M1 writes no
    `instance_settings` row; reconcile when that lands.
