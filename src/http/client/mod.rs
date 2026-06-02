//! Outbound HTTP: a client-agnostic [`UpstreamClient`] trait with a native
//! (wreq) and an edge (fetch) implementation, selected by build target.

use bytes::Bytes;

/// Transport-level error from the upstream client.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("upstream transport error: {0}")]
    Transport(String),
}

/// Client-agnostic upstream HTTP transport. Native impl = wreq (supports TLS
/// emulation); edge impl = host `fetch`. The `?Send` on the wasm async_trait
/// controls the FUTURE; `Send + Sync` here constrains the implementing TYPE so
/// that `Arc<dyn UpstreamClient>` is usable in multi-threaded async contexts.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait UpstreamClient: Send + Sync {
    /// Send a fully-formed request and return the response (status + headers + body bytes).
    async fn send(&self, req: http::Request<Bytes>) -> Result<http::Response<Bytes>, ClientError>;
}

#[cfg(not(target_arch = "wasm32"))]
mod wreq;
#[cfg(not(target_arch = "wasm32"))]
pub use wreq::WreqClient;

#[cfg(target_arch = "wasm32")]
mod fetch;
#[cfg(target_arch = "wasm32")]
pub use fetch::FetchClient;
