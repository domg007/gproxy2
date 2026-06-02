//! Client-agnostic upstream HTTP transport trait.

use bytes::Bytes;

/// Transport-level error from the upstream client.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("upstream transport error: {0}")]
    Transport(String),
}

/// Client-agnostic upstream HTTP transport. Native impl = wreq (supports
/// TLS emulation); edge impl = host fetch. Do NOT add `Send + Sync` here —
/// the edge fetch future is `!Send`.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait UpstreamClient {
    /// Send a fully-formed request and return the response (status + headers + body bytes).
    async fn send(&self, req: http::Request<Bytes>) -> Result<http::Response<Bytes>, ChannelError>;
}
