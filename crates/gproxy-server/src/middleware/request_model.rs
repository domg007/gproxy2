use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use serde_json::Value;
use tower::{Layer, Service};

use crate::middleware::classify::ClassifiedRequest;
use crate::middleware::error::MiddlewareError;
use crate::middleware::kinds::{OperationFamily, ProtocolKind};

/// A classified request enriched with the extracted model identifier.
pub struct ModelScopedRequest {
    pub request: ClassifiedRequest,
    pub model: Option<String>,
}

/// Extract the model identifier from a classified request.
///
/// The model may come from the JSON body (`/model`) or from the URI path
/// (Gemini-style `/models/{model}:action`), depending on the operation and
/// protocol.
pub fn extract_model(req: &ClassifiedRequest) -> Result<Option<String>, MiddlewareError> {
    if req.operation == OperationFamily::ModelList {
        return Ok(None);
    }

    let source = model_source(req.operation, req.protocol);

    match source {
        ModelSource::UriPath => Ok(extract_model_from_uri_path(req.request.uri().path())),
        ModelSource::Body(pointer) => {
            let body = req.request.body();
            if body.is_empty() {
                return Ok(None);
            }
            let value: Value =
                serde_json::from_slice(body).map_err(|e| MiddlewareError::JsonDecode {
                    kind: "request",
                    operation: req.operation,
                    protocol: req.protocol,
                    message: e.to_string(),
                })?;
            Ok(ptr(&value, pointer))
        }
        ModelSource::BodyOrUriPath(pointer) => {
            let body = req.request.body();
            if !body.is_empty()
                && let Ok(value) = serde_json::from_slice::<Value>(body)
                && let Some(model) = ptr(&value, pointer)
            {
                return Ok(Some(model));
            }
            Ok(extract_model_from_uri_path(req.request.uri().path()))
        }
    }
}

/// Wrap a classified request with its extracted model.
pub fn extract_model_from_classified(
    req: ClassifiedRequest,
) -> Result<ModelScopedRequest, MiddlewareError> {
    let model = extract_model(&req)?;
    Ok(ModelScopedRequest {
        request: req,
        model,
    })
}

// ---------------------------------------------------------------------------
// Model source per (operation, protocol)
// ---------------------------------------------------------------------------

enum ModelSource {
    /// Model is in the URI path (Gemini: `/models/{model}:action`).
    UriPath,
    /// Model is in the JSON body at the given pointer.
    Body(&'static str),
    /// Try body pointer first, fall back to URI path.
    BodyOrUriPath(&'static str),
}

fn model_source(op: OperationFamily, proto: ProtocolKind) -> ModelSource {
    match (op, proto) {
        (OperationFamily::ModelList, _) => ModelSource::Body("/model"), // unreachable

        // ModelGet: model identifier is in the URL path for all protocols
        (OperationFamily::ModelGet, _) => ModelSource::UriPath,

        // Gemini: model is always in the URL path
        (
            OperationFamily::GenerateContent | OperationFamily::StreamGenerateContent,
            ProtocolKind::Gemini | ProtocolKind::GeminiNDJson,
        )
        | (OperationFamily::Embedding, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => {
            ModelSource::UriPath
        }

        // Gemini count tokens: might be in body's generate_content_request.model,
        // or fall back to URI path
        (OperationFamily::CountToken, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => {
            ModelSource::BodyOrUriPath("/generate_content_request/model")
        }

        // Gemini Live: model is in body setup.model
        (OperationFamily::GeminiLive, ProtocolKind::Gemini) => ModelSource::Body("/setup/model"),

        // OpenAI / Claude / ChatCompletion — model is in body /model
        _ => ModelSource::Body("/model"),
    }
}

/// Extract model name from a Gemini-style URI path like
/// `/v1beta/models/gemini-pro:generateContent`, or from a model-get path like
/// `/v1/models/gpt-4o`.
pub fn extract_model_from_uri_path(path: &str) -> Option<String> {
    // Normalize: strip /v1, /v1beta, etc.
    let normalized = crate::middleware::classify::normalize_path(path);
    // Expect `/models/{model}` or `/models/{model}:action`
    let tail = normalized.strip_prefix("/models/")?;
    if tail.is_empty() {
        return None;
    }
    // Strip `:action` suffix if present
    let model = tail.split(':').next().unwrap_or(tail);
    if model.is_empty() {
        return None;
    }
    Some(model.to_string())
}

fn ptr(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

// ---------------------------------------------------------------------------
// Tower Layer / Service
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub struct RequestModelExtractLayer;

impl RequestModelExtractLayer {
    pub const fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for RequestModelExtractLayer {
    type Service = RequestModelExtractService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestModelExtractService { inner }
    }
}

#[derive(Debug, Clone)]
pub struct RequestModelExtractService<S> {
    inner: S,
}

#[derive(Debug)]
pub enum RequestModelExtractServiceError<E> {
    Extract(MiddlewareError),
    Inner(E),
}

impl<E: Display> Display for RequestModelExtractServiceError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Extract(err) => Display::fmt(err, f),
            Self::Inner(err) => Display::fmt(err, f),
        }
    }
}

impl<E: Error + 'static> Error for RequestModelExtractServiceError<E> {}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

impl<S> Service<ClassifiedRequest> for RequestModelExtractService<S>
where
    S: Service<ModelScopedRequest> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = RequestModelExtractServiceError<S::Error>;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(RequestModelExtractServiceError::Inner)
    }

    fn call(&mut self, request: ClassifiedRequest) -> Self::Future {
        let mut inner = self.inner.clone();
        Box::pin(async move {
            let scoped = extract_model_from_classified(request)
                .map_err(RequestModelExtractServiceError::Extract)?;
            inner
                .call(scoped)
                .await
                .map_err(RequestModelExtractServiceError::Inner)
        })
    }
}
