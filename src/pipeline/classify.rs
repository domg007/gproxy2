//! Inbound request classification: `(method, path)` → [`OperationKey`] plus the
//! streaming flag. M1 ships a hardcoded table (D5); unknown rows → 404.

use bytes::Bytes;
use http::Method;

use crate::pipeline::context::Classified;
use crate::pipeline::error::PipelineError;
use crate::protocol::{ContentGenerationKind as CGK, Operation, OperationKey, Provider as Prov};

/// Classify by `(method, path)`. The leading `/v1` is present in both aggregated
/// and (post-strip) scoped paths.
pub fn classify(method: &Method, path: &str, body: &Bytes) -> Result<Classified, PipelineError> {
    let (op, stream) = match (method.as_str(), path) {
        ("POST", "/v1/chat/completions") => (
            OperationKey::content_generation(
                Operation::GenerateContent,
                CGK::OpenAiChatCompletions,
            ),
            peek_stream(body),
        ),
        ("POST", "/v1/responses") => (
            OperationKey::content_generation(Operation::GenerateContent, CGK::OpenAiResponses),
            peek_stream(body),
        ),
        ("POST", "/v1/messages") => (
            OperationKey::content_generation(Operation::GenerateContent, CGK::ClaudeMessages),
            peek_stream(body),
        ),
        ("POST", "/v1/embeddings") => (
            OperationKey::provider(Operation::CreateEmbedding, Prov::OpenAi),
            false,
        ),
        ("POST", "/v1/images/generations") => (
            OperationKey::provider(Operation::CreateImage, Prov::OpenAi),
            false,
        ),
        ("POST", p) => match gemini_suffix(p) {
            Some(key_stream) => key_stream,
            None => return Err(PipelineError::UnsupportedPath),
        },
        _ => return Err(PipelineError::UnsupportedPath),
    };
    Ok(Classified { op, stream })
}

/// Gemini `…/models/{model}:generateContent` / `:streamGenerateContent`, matched
/// on the path suffix after the last `/` (independent of `{model}`).
fn gemini_suffix(path: &str) -> Option<(OperationKey, bool)> {
    let last = path.rsplit('/').next()?;
    if last.ends_with(":streamGenerateContent") {
        Some((
            OperationKey::content_generation(
                Operation::StreamGenerateContent,
                CGK::GeminiGenerateContent,
            ),
            true,
        ))
    } else if last.ends_with(":generateContent") {
        Some((
            OperationKey::content_generation(
                Operation::GenerateContent,
                CGK::GeminiGenerateContent,
            ),
            false,
        ))
    } else {
        None
    }
}

/// Minimal body peek for the `"stream"` flag (NOT a full protocol deserialize).
/// Tolerant: a type error elsewhere in the body must not flip the flag, and a
/// non-bool `stream` is treated as absent.
pub(crate) fn peek_stream(body: &Bytes) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("stream").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// Minimal body peek for the `"model"` field (tolerant, as above).
pub(crate) fn peek_model(body: &Bytes) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("model").and_then(|m| m.as_str().map(str::to_string)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_completions_streaming() {
        let body = Bytes::from_static(b"{\"model\":\"x\",\"stream\":true}");
        let c = classify(&Method::POST, "/v1/chat/completions", &body).unwrap();
        assert!(c.stream);
    }

    #[test]
    fn gemini_stream_suffix() {
        let body = Bytes::new();
        let c = classify(
            &Method::POST,
            "/v1beta/models/gemini-pro:streamGenerateContent",
            &body,
        )
        .unwrap();
        assert!(c.stream);
    }

    #[test]
    fn unknown_path_is_unsupported() {
        let body = Bytes::new();
        assert!(matches!(
            classify(&Method::POST, "/v1/nope", &body),
            Err(PipelineError::UnsupportedPath)
        ));
    }
}
