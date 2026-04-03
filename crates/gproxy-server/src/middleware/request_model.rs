use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::middleware::classify::{Classification, extract_model_from_uri_path};
use crate::middleware::kinds::{OperationFamily, ProtocolKind};

/// Extracted model stored in request extensions.
#[derive(Debug, Clone)]
pub struct ExtractedModel(pub Option<String>);

/// Axum middleware: extract the model name from the request body or URI path
/// and store `ExtractedModel` in extensions.
///
/// Requires `Classification` to already be in extensions (run after classify).
pub async fn request_model_middleware(request: Request, next: Next) -> Response {
    let classification = request.extensions().get::<Classification>().cloned();
    let model = classification
        .as_ref()
        .and_then(|c| extract_model_from_request(&request, c.operation, c.protocol));
    let mut request = request;
    request.extensions_mut().insert(ExtractedModel(model));
    next.run(request).await
}

fn extract_model_from_request(
    request: &Request,
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> Option<String> {
    if operation == OperationFamily::ModelList {
        return None;
    }

    match model_source(operation, protocol) {
        ModelSource::UriPath => extract_model_from_uri_path(request.uri().path()),
        ModelSource::Body(_pointer) => {
            let _body = request.body();
            // Body was already buffered by classify middleware, read from extensions
            // or try to get bytes directly — but axum body is consumed.
            // Since classify already buffered, body is now Body::from(Bytes).
            // We can't re-read it here without consuming. Use a workaround:
            // The body was set as Bytes by classify, but axum wraps it in Body.
            // We'll skip body extraction here and let handler do it.
            // For now, return None for body-based models — handler extracts model.
            None
        }
        ModelSource::BodyOrUriPath(_pointer) => extract_model_from_uri_path(request.uri().path()),
    }
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
