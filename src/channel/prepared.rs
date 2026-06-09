//! The output of [`Channel::prepare`](crate::channel::Channel::prepare).

use bytes::Bytes;

/// A fully-addressed, auth-injected upstream request ready for
/// [`UpstreamClient::send`](crate::http::client::UpstreamClient::send).
#[derive(Debug)]
pub struct PreparedRequest {
    /// `request.uri()` MUST be absolute (scheme + authority + path + query) —
    /// wreq cannot route a relative URI (see [`http_util::join_url`]).
    ///
    /// [`http_util::join_url`]: crate::channel::http_util::join_url
    pub request: http::Request<Bytes>,
    /// Per-attempt outbound proxy override (credential proxy ?? provider default,
    /// §7.4). Native only; ignored on edge and **ignored in M1** — carried for
    /// the future `(proxy_url, tls_emulation)`-keyed client pool.
    pub proxy_url: Option<String>,
}

impl PreparedRequest {
    /// Consume into the bare `http::Request` for the transport.
    pub fn into_http(self) -> http::Request<Bytes> {
        self.request
    }
}
