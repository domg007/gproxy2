//! Native (wreq) implementation of [`UpstreamClient`].
//!
//! Translates `http::Request<Bytes>` -> wreq request -> `http::Response<Bytes>`.

use bytes::Bytes;

use super::{ClientError, UpstreamClient};

/// Upstream client backed by [`wreq::Client`] (native, TLS-emulation capable).
#[derive(Clone)]
pub struct WreqClient {
    inner: wreq::Client,
}

impl WreqClient {
    /// Build a `WreqClient` with a default [`wreq::Client`].
    ///
    /// Auto-decompression (`gzip`/`brotli`/`zstd`/`deflate`) is enabled: the
    /// client transparently inflates compressed responses, so downstream
    /// transform/billing logic always sees the plaintext body.
    pub fn new() -> Self {
        Self {
            inner: wreq::Client::new(),
        }
    }

    /// Build a `WreqClient` with an optional all-traffic upstream proxy.
    pub fn with_proxy_url(proxy_url: Option<&str>) -> wreq::Result<Self> {
        let mut builder = wreq::Client::builder();
        if let Some(proxy_url) = proxy_url {
            builder = builder.proxy(wreq::Proxy::all(proxy_url)?);
        }
        Ok(Self {
            inner: builder.build()?,
        })
    }
}

impl Default for WreqClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl UpstreamClient for WreqClient {
    async fn send(&self, req: http::Request<Bytes>) -> Result<http::Response<Bytes>, ClientError> {
        // http::Request<Bytes> -> wreq::Request via From impl (Bytes: Into<wreq::Body>).
        let wreq_req: wreq::Request = req.into();

        let resp = self
            .inner
            .execute(wreq_req)
            .await
            .map_err(|e| ClientError::Transport(e.to_string()))?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let body_bytes = resp
            .bytes()
            .await
            .map_err(|e| ClientError::Transport(e.to_string()))?;

        let mut builder = http::Response::builder().status(status);
        if let Some(hmap) = builder.headers_mut() {
            *hmap = headers;
        }
        builder
            .body(body_bytes)
            .map_err(|e| ClientError::Transport(e.to_string()))
    }
}
