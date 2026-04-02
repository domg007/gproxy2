/// Upstream response from a channel.
#[derive(Debug)]
pub struct UpstreamResponse {
    pub status: u16,
    pub headers: http::HeaderMap,
    pub body: Vec<u8>,
}

/// Classification of an upstream response for retry decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseClassification {
    /// 2xx — request succeeded.
    Success,
    /// 401/403 — credential permanently invalid.
    AuthDead,
    /// 429 — rate limited, retry with another credential.
    RateLimited { retry_after_ms: Option<u64> },
    /// 5xx transient — server error, worth retrying.
    TransientError,
    /// Other error — not worth retrying.
    PermanentError,
}

/// Error from upstream channel execution.
#[derive(Debug, thiserror::Error)]
pub enum UpstreamError {
    #[error("all credentials exhausted")]
    AllCredentialsExhausted,
    #[error("no eligible credentials")]
    NoEligibleCredentials,
    #[error("request build error: {0}")]
    RequestBuild(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("channel error: {0}")]
    Channel(String),
}
