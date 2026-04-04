use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::middleware::classify::{BufferedBodyBytes, Classification, extract_model_from_uri_path};
use crate::middleware::kinds::{OperationFamily, ProtocolKind};

/// Extracted model stored in request extensions.
#[derive(Debug, Clone)]
pub struct ExtractedModel(pub Option<String>);

/// Axum middleware: extract the model name from the request body or URI path
/// and store `ExtractedModel` in extensions.
///
/// Requires `Classification` and `BufferedBodyBytes` to already be in extensions
/// (run after classify_middleware).
pub async fn request_model_middleware(request: Request, next: Next) -> Response {
    let classification = request.extensions().get::<Classification>().cloned();
    let body_bytes = request.extensions().get::<BufferedBodyBytes>().cloned();
    let model = classification
        .as_ref()
        .and_then(|c| extract_model_from_request(&request, body_bytes.as_ref(), c.operation, c.protocol));
    let mut request = request;
    request.extensions_mut().insert(ExtractedModel(model));
    next.run(request).await
}

fn extract_model_from_request(
    request: &Request,
    body_bytes: Option<&BufferedBodyBytes>,
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> Option<String> {
    if operation == OperationFamily::ModelList {
        return None;
    }

    match model_source(operation, protocol) {
        ModelSource::UriPath => extract_model_from_uri_path(request.uri().path()),
        ModelSource::Body(pointer) => extract_model_from_body(body_bytes, pointer),
        ModelSource::BodyOrUriPath(pointer) => extract_model_from_uri_path(request.uri().path())
            .or_else(|| extract_model_from_body(body_bytes, pointer)),
    }
}

fn extract_model_from_body(body_bytes: Option<&BufferedBodyBytes>, pointer: &str) -> Option<String> {
    let bytes = &body_bytes?.0;
    if bytes.is_empty() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let value = json.pointer(pointer)?;
    value.as_str().map(|s| s.to_string())
}

enum ModelSource {
    UriPath,
    Body(&'static str),
    BodyOrUriPath(&'static str),
}

fn model_source(op: OperationFamily, proto: ProtocolKind) -> ModelSource {
    match (op, proto) {
        (OperationFamily::ModelList, _) => ModelSource::Body("/model"),
        (OperationFamily::ModelGet, _) => ModelSource::UriPath,
        (
            OperationFamily::GenerateContent | OperationFamily::StreamGenerateContent,
            ProtocolKind::Gemini | ProtocolKind::GeminiNDJson,
        )
        | (OperationFamily::Embedding, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => {
            ModelSource::UriPath
        }
        (OperationFamily::CountToken, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => {
            ModelSource::BodyOrUriPath("/generate_content_request/model")
        }
        (OperationFamily::GeminiLive, ProtocolKind::Gemini) => ModelSource::Body("/setup/model"),
        _ => ModelSource::Body("/model"),
    }
}
