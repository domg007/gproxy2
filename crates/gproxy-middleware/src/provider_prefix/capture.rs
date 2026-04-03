use bytes::Bytes;
use serde_json::Value;

use crate::classify::ClassifiedRequest;
use crate::error::MiddlewareError;
use crate::kinds::{OperationFamily, ProtocolKind};

// ---------------------------------------------------------------------------
// Public utilities
// ---------------------------------------------------------------------------

/// Split a model string into `(has_models_prefix, provider, model_without_provider)`.
///
/// Examples:
/// - `"openai/gpt-4o"` → `Some((false, "openai", "gpt-4o"))`
/// - `"models/openai/gemini-pro"` → `Some((true, "openai", "gemini-pro"))`
/// - `"gpt-4o"` → `None` (no provider prefix)
pub fn split_provider_prefixed_model(value: &str) -> Option<(bool, &str, &str)> {
    let (has_models_prefix, tail) = if let Some(rest) = value.strip_prefix("models/") {
        (true, rest)
    } else {
        (false, value)
    };
    let (provider, model) = tail.split_once('/')?;
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some((has_models_prefix, provider, model))
}

/// Add a provider prefix to a model string.
///
/// - `("gpt-4o", "openai")` → `"openai/gpt-4o"`
/// - `("models/gemini-pro", "vertex")` → `"models/vertex/gemini-pro"`
/// - Already prefixed values are returned as-is.
pub fn add_provider_prefix(value: &str, provider: &str) -> String {
    if provider.is_empty() || split_provider_prefixed_model(value).is_some() {
        return value.to_string();
    }
    if let Some(rest) = value.strip_prefix("models/") {
        return format!("models/{provider}/{rest}");
    }
    if value.is_empty() {
        provider.to_string()
    } else {
        format!("{provider}/{value}")
    }
}

// ---------------------------------------------------------------------------
// Request-side: strip provider prefix from ClassifiedRequest body
// ---------------------------------------------------------------------------

/// Strip the provider prefix from the model field(s) in a classified request.
/// Returns `Some(provider)` if a prefix was found, `None` otherwise.
///
/// The request body is mutated in-place (the `ClassifiedRequest.request` body
/// is replaced with an updated copy).
pub(super) fn strip_provider_prefix_from_classified(
    req: &mut ClassifiedRequest,
) -> Result<Option<String>, MiddlewareError> {
    // ModelList has no model field
    if req.operation == OperationFamily::ModelList {
        return Ok(None);
    }

    let mut capture = ProviderCapture::new();
    let pointers = body_model_pointers(req.operation, req.protocol);

    // --- Body-based model fields ---
    if !pointers.is_empty() {
        let body = req.request.body();
        if !body.is_empty() {
            let mut value: Value =
                serde_json::from_slice(body).map_err(|e| MiddlewareError::JsonDecode {
                    kind: "request",
                    operation: req.operation,
                    protocol: req.protocol,
                    message: e.to_string(),
                })?;

            for pointer in pointers {
                let Some(slot) = value.pointer_mut(pointer) else {
                    continue;
                };
                let Some(raw) = slot.as_str() else {
                    continue;
                };
                let Some((has_models, provider, model)) = split_provider_prefixed_model(raw) else {
                    continue;
                };
                if let Some(existing) = capture.provider.as_ref() {
                    if existing != provider {
                        return Err(MiddlewareError::ProviderPrefix {
                            message: format!(
                                "inconsistent provider prefix: expected {existing}, got {provider}"
                            ),
                        });
                    }
                } else {
                    capture.provider = Some(provider.to_string());
                }
                *slot = Value::String(if has_models {
                    format!("models/{model}")
                } else {
                    model.to_string()
                });
            }

            if capture.provider.is_some() {
                let new_body = serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec());
                *req.request.body_mut() = Bytes::from(new_body);
            }
        }
    }

    // --- URI path-based model (Gemini, ModelGet) ---
    if capture.provider.is_none()
        && let Some(model_in_path) =
            crate::request_model::extract_model_from_uri_path(req.request.uri().path())
        && let Some((has_models, provider, model)) = split_provider_prefixed_model(&model_in_path)
    {
        capture.provider = Some(provider.to_string());
        // Rewrite URI path: replace the model segment
        let old_segment = if has_models {
            format!("models/{model_in_path}")
        } else {
            model_in_path.clone()
        };
        let new_segment = if has_models {
            format!("models/{model}")
        } else {
            model.to_string()
        };
        let old_uri = req.request.uri().to_string();
        if let Ok(new_uri) = old_uri.replace(&old_segment, &new_segment).parse() {
            *req.request.uri_mut() = new_uri;
        }
    }

    Ok(capture.provider)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

struct ProviderCapture {
    provider: Option<String>,
}

impl ProviderCapture {
    fn new() -> Self {
        Self { provider: None }
    }
}

/// Return the JSON pointer(s) where the model field lives in the **body**
/// for a given (operation, protocol) pair.  Returns an empty slice when the
/// model is not in the body (e.g. Gemini path-based model).
///
/// Note: Gemini's model-in-URL-path is handled separately — provider prefix
/// stripping for path-based models is done directly on the URI, not via JSON
/// pointers.
fn body_model_pointers(op: OperationFamily, proto: ProtocolKind) -> &'static [&'static str] {
    match (op, proto) {
        // Gemini: model is in the URL path, not in body
        (OperationFamily::ModelGet, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => &[],
        (
            OperationFamily::GenerateContent | OperationFamily::StreamGenerateContent,
            ProtocolKind::Gemini | ProtocolKind::GeminiNDJson,
        ) => &[],
        (OperationFamily::Embedding, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => &[],
        // ModelGet for OpenAI/Claude: model is in URL path, not body
        (OperationFamily::ModelGet, _) => &[],

        // Gemini count tokens: model may be in body
        (OperationFamily::CountToken, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => {
            &["/generate_content_request/model"]
        }
        // Gemini Live: model in setup.model
        (OperationFamily::GeminiLive, ProtocolKind::Gemini) => &["/setup/model"],

        // OpenAI / Claude / ChatCompletion — model is always /model
        _ => &["/model"],
    }
}
