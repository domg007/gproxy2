//! Channel layer — pure upstream *access* (§6.1, §6.3).
//!
//! A `Channel` injects auth, resolves the endpoint, and declares transport
//! capability. It does **no** protocol transform and **no** rule rewriting
//! (those are the transform/process layers) and never mutates the request body.

pub mod bulletins;
pub mod disposition;
pub mod envelope;
pub mod http_util;
pub mod login;
pub mod oauth;
pub mod prepared;
pub mod registry;
pub mod resolve;

use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use serde_json::Value;

use crate::http::client::UpstreamClient;
use crate::protocol::ContentGenerationKind;

pub use disposition::Disposition;
pub use login::{AuthCodeStart, ChannelLogin};
pub use prepared::PreparedRequest;

/// Declared upstream transport, for capability-based degradation (§7.4).
/// M1 uses `Http` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Http,
    Ws,
}

/// A per-channel byte-stream decoder spliced BEFORE the M2 protocol transform.
/// Used by envelope/binary channels (code-assist per-frame unwrap, kiro Smithy
/// → SSE). Sync core, mirrors the protocol `SseTransformer`
/// ([`crate::transform::stream_adapter::SseTransformer`]): `push` per upstream
/// chunk, `finish` at EOF. Streaming is native-only, but the trait is defined
/// on both targets (the hook returns `None` everywhere by default) — the `Send`
/// bound is harmless on wasm since the decoder is never held across an await.
pub trait ChannelStreamDecoder: Send {
    /// Feed one raw upstream chunk; return decoded bytes (possibly empty while a
    /// frame is still buffering).
    fn push(&mut self, chunk: &[u8]) -> Vec<u8>;
    /// Flush any trailing buffered state at end of stream.
    fn finish(&mut self) -> Vec<u8>;
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
    /// Member rewrite target. PATH construction only (path-templated
    /// providers); body model rewrite happens in the pipeline transform
    /// step before prepare.
    pub upstream_model_id: &'a str,
    pub method: http::Method,
    /// Inbound, provider-relative path (`/v1/...`); scoped mode already stripped
    /// of the leading `/{provider}`.
    pub path: &'a str,
    pub query: Option<&'a str>,
    /// Inbound headers (the channel sanitizes + injects its own auth).
    pub headers: &'a HeaderMap,
    /// Effective upstream body (post-transform/process; verbatim on
    /// passthrough).
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

    /// Optional channel-specific stream decoder (envelope unwrap / binary →
    /// SSE), applied to the raw upstream byte stream before any protocol
    /// transform. Default: none (passthrough).
    fn stream_decoder(&self) -> Option<Box<dyn ChannelStreamDecoder>> {
        None
    }

    /// Whether the DECRYPTED secret must be refreshed before use (e.g. OAuth
    /// access token near expiry). Default: never.
    fn needs_refresh(&self, _secret: &Value) -> bool {
        false
    }

    /// Refresh the credential against the provider, returning the new PLAINTEXT
    /// secret Value. The pipeline re-seals + persists + publishes — the channel
    /// never touches cipher/persistence (purity §6.3). Default: unsupported.
    async fn refresh(
        &self,
        _client: &Arc<dyn UpstreamClient>,
        _secret: &Value,
    ) -> Result<Value, ChannelError> {
        Err(ChannelError::Unsupported("refresh"))
    }

    fn transport(&self) -> TransportKind {
        TransportKind::Http
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
