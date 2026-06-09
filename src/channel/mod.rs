//! Channel layer — pure upstream *access* (§6.1, §6.3).
//!
//! A `Channel` injects auth, resolves the endpoint, and declares transport
//! capability. It does **no** protocol transform and **no** rule rewriting
//! (those are the transform/process layers) and never mutates the request body.

pub mod bulletins;
pub mod disposition;
pub mod http_util;
pub mod prepared;
pub mod registry;

use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use serde_json::Value;

use crate::http::client::UpstreamClient;
use crate::protocol::ContentGenerationKind;
use crate::store::persistence::records::Credential;

pub use disposition::Disposition;
pub use prepared::PreparedRequest;

/// Declared upstream transport, for capability-based degradation (§7.4).
/// M1 uses `Http` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Http,
    Ws,
}

/// Per-call inputs the channel needs to build the upstream request.
///
/// The `'a` fields borrow snapshot-owned data; `body` is owned and is **moved**
/// into the constructed request by `prepare` (channel impls must move it).
pub struct PrepareCtx<'a> {
    /// Decrypted secret material (M1: plaintext; envelope decryption in M6).
    pub secret: &'a Value,
    /// Provider settings (`base_url`, channel toggles, …).
    pub provider_settings: &'a Value,
    /// Member rewrite target. M1: PATH construction only (path-templated
    /// providers); NEVER used to mutate the body.
    pub upstream_model_id: &'a str,
    pub method: http::Method,
    /// Inbound, provider-relative path (`/v1/...`); scoped mode already stripped
    /// of the leading `/{provider}`.
    pub path: &'a str,
    pub query: Option<&'a str>,
    /// Inbound headers (the channel sanitizes + injects its own auth).
    pub headers: &'a HeaderMap,
    /// Inbound body, forwarded verbatim (same-protocol passthrough).
    pub body: Bytes,
}

/// Pure upstream access adapter (§6.3). Implementors provide `id`, `target_kind`
/// and `prepare`; the rest have sensible defaults.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait Channel: Send + Sync {
    /// Stable channel id used as the registry key (matches `Provider.channel`).
    fn id(&self) -> &'static str;

    /// The native content-generation wire format this channel speaks. Pins the
    /// M2 transform-bypass predicate (`source_kind == target_kind`) at M1 time.
    fn target_kind(&self) -> ContentGenerationKind;

    /// Inject auth, resolve endpoint + method, set an ABSOLUTE upstream URL.
    /// Pure access — no transform/rules, no body mutation. Moves `ctx.body` in.
    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError>;

    /// Map an upstream response to the 5-state [`Disposition`]. Default is the
    /// generic HTTP-status mapping; override only for provider-specific signals.
    /// For streaming, `body` is empty (status + headers suffice).
    fn classify(&self, status: StatusCode, headers: &HeaderMap, _body: &Bytes) -> Disposition {
        Disposition::from_http(status, headers)
    }

    /// Channel-specific fixups on the raw upstream body before transform.
    /// M1 same-protocol: identity.
    fn normalize(&self, body: Bytes) -> Bytes {
        body
    }

    /// Whether the credential must be refreshed before use. M1: never.
    fn needs_refresh(&self, _cred: &Credential) -> bool {
        false
    }

    /// Refresh the credential against the provider. M1: unsupported.
    async fn refresh(
        &self,
        _client: &Arc<dyn UpstreamClient>,
        _cred: &Credential,
    ) -> Result<Value, ChannelError> {
        Err(ChannelError::Unsupported("refresh"))
    }

    fn transport(&self) -> TransportKind {
        TransportKind::Http
    }

    fn requires_tls_emulation(&self) -> bool {
        false
    }
}

/// Errors raised while preparing an upstream request.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("missing setting: {0}")]
    MissingSetting(&'static str),
    #[error("invalid credential: {0}")]
    InvalidCredential(String),
    #[error("unsupported: {0}")]
    Unsupported(&'static str),
    #[error("build error: {0}")]
    Build(String),
}
