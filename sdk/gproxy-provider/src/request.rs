/// A prepared upstream request, protocol-agnostic.
#[derive(Debug, Clone)]
pub struct PreparedRequest {
    /// HTTP method.
    pub method: http::Method,
    /// Request path (e.g. "/v1/chat/completions").
    pub path: String,
    /// Target model name (if known).
    pub model: Option<String>,
    /// Request body bytes.
    pub body: Vec<u8>,
    /// Extra headers to forward.
    pub headers: http::HeaderMap,
}
