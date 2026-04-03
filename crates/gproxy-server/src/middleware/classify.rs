use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use http::{HeaderMap, Method, Request};
use serde::Deserialize;
use tower::{Layer, Service};

use crate::middleware::error::MiddlewareError;
use crate::middleware::kinds::{OperationFamily, ProtocolKind};

/// Raw HTTP request with a buffered body.
pub type ClassifyRequest = Request<Bytes>;

/// Result of request classification: the original request plus the detected
/// operation family and wire protocol.
#[derive(Debug)]
pub struct ClassifiedRequest {
    pub request: ClassifyRequest,
    pub operation: OperationFamily,
    pub protocol: ProtocolKind,
}

/// Classify a raw HTTP request into an operation family and protocol kind.
pub fn classify_request_payload(
    input: ClassifyRequest,
) -> Result<ClassifiedRequest, MiddlewareError> {
    let route = classify_route(&input)?;
    Ok(ClassifiedRequest {
        request: input,
        operation: route.operation,
        protocol: route.protocol,
    })
}

// ---------------------------------------------------------------------------
// Internal classification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct ClassifiedRoute {
    operation: OperationFamily,
    protocol: ProtocolKind,
}

fn classify_route(input: &ClassifyRequest) -> Result<ClassifiedRoute, MiddlewareError> {
    let path = normalize_path(input.uri().path());
    let query = input.uri().query();
    let method = input.method();
    let headers = input.headers();

    if *method == Method::GET {
        if path == "/models" {
            return Ok(ClassifiedRoute {
                operation: OperationFamily::ModelList,
                protocol: classify_models_protocol(headers, query),
            });
        }
        if is_model_get_path(&path) {
            return Ok(ClassifiedRoute {
                operation: OperationFamily::ModelGet,
                protocol: classify_models_protocol(headers, query),
            });
        }
        return Err(MiddlewareError::Unsupported(
            "unsupported GET request path for classification",
        ));
    }

    if *method != Method::POST {
        return Err(MiddlewareError::Unsupported(
            "unsupported HTTP method for classification",
        ));
    }

    // -- OpenAI Responses API ------------------------------------------------
    if path == "/responses" {
        return Ok(ClassifiedRoute {
            operation: stream_or_non_stream(input.body()),
            protocol: ProtocolKind::OpenAi,
        });
    }

    // -- OpenAI Chat Completions ---------------------------------------------
    if path == "/chat/completions" {
        return Ok(ClassifiedRoute {
            operation: stream_or_non_stream(input.body()),
            protocol: ProtocolKind::OpenAiChatCompletion,
        });
    }

    // -- Claude Messages -----------------------------------------------------
    if path == "/messages" {
        return Ok(ClassifiedRoute {
            operation: stream_or_non_stream(input.body()),
            protocol: ProtocolKind::Claude,
        });
    }

    // -- Token counting ------------------------------------------------------
    if path == "/responses/input_tokens" || path == "/responses/input_tokens/count" {
        return Ok(ClassifiedRoute {
            operation: OperationFamily::CountToken,
            protocol: ProtocolKind::OpenAi,
        });
    }
    if path == "/messages/count_tokens" || path == "/messages/count-tokens" {
        return Ok(ClassifiedRoute {
            operation: OperationFamily::CountToken,
            protocol: ProtocolKind::Claude,
        });
    }

    // -- Compact -------------------------------------------------------------
    if path == "/responses/compact" {
        return Ok(ClassifiedRoute {
            operation: OperationFamily::Compact,
            protocol: ProtocolKind::OpenAi,
        });
    }

    // -- Embeddings ----------------------------------------------------------
    if path == "/embeddings" {
        return Ok(ClassifiedRoute {
            operation: OperationFamily::Embedding,
            protocol: ProtocolKind::OpenAi,
        });
    }

    // -- Images --------------------------------------------------------------
    if path == "/images/generations" {
        return Ok(ClassifiedRoute {
            operation: if read_stream_flag(input.body()) {
                OperationFamily::StreamCreateImage
            } else {
                OperationFamily::CreateImage
            },
            protocol: ProtocolKind::OpenAi,
        });
    }
    if path == "/images/edits" {
        return Ok(ClassifiedRoute {
            operation: if read_stream_flag(input.body()) {
                OperationFamily::StreamCreateImageEdit
            } else {
                OperationFamily::CreateImageEdit
            },
            protocol: ProtocolKind::OpenAi,
        });
    }

    // -- Gemini paths --------------------------------------------------------
    if let Some((operation, protocol)) = classify_gemini(&path, query) {
        return Ok(ClassifiedRoute {
            operation,
            protocol,
        });
    }

    Err(MiddlewareError::Unsupported(
        "unable to classify request operation/protocol from method/path/query/headers/body",
    ))
}

// ---------------------------------------------------------------------------
// Protocol detection helpers
// ---------------------------------------------------------------------------

fn classify_models_protocol(headers: &HeaderMap, query: Option<&str>) -> ProtocolKind {
    if headers.contains_key("anthropic-version")
        || headers.contains_key("anthropic-beta")
        || headers.contains_key("x-api-key")
        || query_has_key(query, "after_id")
        || query_has_key(query, "before_id")
        || query_has_key(query, "limit")
    {
        return ProtocolKind::Claude;
    }
    if headers.contains_key("x-goog-api-key")
        || query_has_key(query, "pageSize")
        || query_has_key(query, "pageToken")
        || query_has_key(query, "key")
    {
        return ProtocolKind::Gemini;
    }
    ProtocolKind::OpenAi
}

fn classify_gemini(path: &str, query: Option<&str>) -> Option<(OperationFamily, ProtocolKind)> {
    let tail = path.strip_prefix("/models/")?;
    let (_, action) = tail.rsplit_once(':')?;
    match action {
        "countTokens" => Some((OperationFamily::CountToken, ProtocolKind::Gemini)),
        "generateContent" => Some((OperationFamily::GenerateContent, ProtocolKind::Gemini)),
        "streamGenerateContent" => Some((
            OperationFamily::StreamGenerateContent,
            if query_has_value(query, "alt", "sse") {
                ProtocolKind::Gemini
            } else {
                ProtocolKind::GeminiNDJson
            },
        )),
        "embedContent" => Some((OperationFamily::Embedding, ProtocolKind::Gemini)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Path / query / body utilities
// ---------------------------------------------------------------------------

fn is_model_get_path(path: &str) -> bool {
    let Some(tail) = path.strip_prefix("/models/") else {
        return false;
    };
    !tail.is_empty() && !tail.contains('/') && !tail.contains(':')
}

pub fn normalize_path(path: &str) -> String {
    let mut out = if path.starts_with('/') {
        path.trim().to_string()
    } else {
        format!("/{}", path.trim())
    };
    while out.contains("//") {
        out = out.replace("//", "/");
    }
    if out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    for prefix in ["/v1", "/v1beta", "/v1beta1"] {
        if out == prefix {
            return "/".to_string();
        }
        let full = format!("{prefix}/");
        if let Some(rest) = out.strip_prefix(&full) {
            return format!("/{}", rest.trim_start_matches('/'));
        }
    }
    out
}

fn query_has_key(query: Option<&str>, key: &str) -> bool {
    query.is_some_and(|q| q.split('&').any(|pair| pair.split('=').next() == Some(key)))
}

fn query_has_value(query: Option<&str>, key: &str, value: &str) -> bool {
    query.is_some_and(|q| {
        q.split('&').any(|pair| {
            let mut it = pair.splitn(2, '=');
            it.next() == Some(key) && it.next().is_some_and(|v| v.eq_ignore_ascii_case(value))
        })
    })
}

fn stream_or_non_stream(body: &Bytes) -> OperationFamily {
    if read_stream_flag(body) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    }
}

fn read_stream_flag(body: &Bytes) -> bool {
    #[derive(Deserialize)]
    struct StreamFlagBody {
        #[serde(default)]
        stream: Option<bool>,
    }
    if body.is_empty() {
        return false;
    }
    serde_json::from_slice::<StreamFlagBody>(body)
        .ok()
        .and_then(|v| v.stream)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tower Layer / Service
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestClassifyLayer;

impl RequestClassifyLayer {
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for RequestClassifyLayer {
    type Service = RequestClassifyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestClassifyService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct RequestClassifyService<S> {
    inner: S,
}

#[derive(Debug)]
pub enum RequestClassifyServiceError<E> {
    Classify(MiddlewareError),
    Inner(E),
}

impl<E: Display> Display for RequestClassifyServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Classify(err) => Display::fmt(err, f),
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for RequestClassifyServiceError<E> {}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

impl<S> Service<ClassifyRequest> for RequestClassifyService<S>
where
    S: Service<ClassifiedRequest> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = RequestClassifyServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(RequestClassifyServiceError::Inner)
    }

    fn call(&mut self, request: ClassifyRequest) -> Self::Future {
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let classified =
                classify_request_payload(request).map_err(RequestClassifyServiceError::Classify)?;
            inner
                .call(classified)
                .await
                .map_err(RequestClassifyServiceError::Inner)
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(method: Method, uri: &str, body: &str) -> ClassifiedRequest {
        classify_request_payload(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Bytes::from(body.to_string()))
                .expect("request"),
        )
        .expect("classified request")
    }

    #[test]
    fn openai_responses_non_stream() {
        let c = classify(Method::POST, "/v1/responses", r#"{"model":"gpt-4o"}"#);
        assert_eq!(c.operation, OperationFamily::GenerateContent);
        assert_eq!(c.protocol, ProtocolKind::OpenAi);
    }

    #[test]
    fn openai_responses_stream() {
        let c = classify(
            Method::POST,
            "/v1/responses",
            r#"{"model":"gpt-4o","stream":true}"#,
        );
        assert_eq!(c.operation, OperationFamily::StreamGenerateContent);
        assert_eq!(c.protocol, ProtocolKind::OpenAi);
    }

    #[test]
    fn claude_messages() {
        let c = classify(
            Method::POST,
            "/v1/messages",
            r#"{"model":"claude-4-sonnet","stream":true}"#,
        );
        assert_eq!(c.operation, OperationFamily::StreamGenerateContent);
        assert_eq!(c.protocol, ProtocolKind::Claude);
    }

    #[test]
    fn gemini_stream_sse() {
        let c = classify(
            Method::POST,
            "/v1beta/models/gemini-pro:streamGenerateContent?alt=sse",
            "{}",
        );
        assert_eq!(c.operation, OperationFamily::StreamGenerateContent);
        assert_eq!(c.protocol, ProtocolKind::Gemini);
    }

    #[test]
    fn gemini_stream_ndjson() {
        let c = classify(
            Method::POST,
            "/v1beta/models/gemini-pro:streamGenerateContent",
            "{}",
        );
        assert_eq!(c.operation, OperationFamily::StreamGenerateContent);
        assert_eq!(c.protocol, ProtocolKind::GeminiNDJson);
    }

    #[test]
    fn model_list_openai() {
        let c = classify(Method::GET, "/v1/models", "");
        assert_eq!(c.operation, OperationFamily::ModelList);
        assert_eq!(c.protocol, ProtocolKind::OpenAi);
    }

    #[test]
    fn image_generation_stream() {
        let c = classify(
            Method::POST,
            "/v1/images/generations",
            r#"{"prompt":"demo","stream":true}"#,
        );
        assert_eq!(c.operation, OperationFamily::StreamCreateImage);
        assert_eq!(c.protocol, ProtocolKind::OpenAi);
    }
}
