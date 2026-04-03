use crate::middleware::classify::extract_model_from_uri_path;
use crate::middleware::kinds::{OperationFamily, ProtocolKind};

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

/// Strip provider prefix from a model string in a JSON body.
/// Returns `(provider, stripped_body)` if a prefix was found.
pub fn strip_provider_from_body(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Option<(String, Vec<u8>)> {
    let pointers = body_model_pointers(operation, protocol);
    if pointers.is_empty() || body.is_empty() {
        return None;
    }
    let mut value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let mut provider: Option<String> = None;

    for pointer in pointers {
        let Some(slot) = value.pointer_mut(pointer) else {
            continue;
        };
        let Some(raw) = slot.as_str() else { continue };
        let Some((has_models, prov, model)) = split_provider_prefixed_model(raw) else {
            continue;
        };
        if let Some(existing) = &provider {
            if existing != prov {
                return None; // inconsistent
            }
        } else {
            provider = Some(prov.to_string());
        }
        *slot = serde_json::Value::String(if has_models {
            format!("models/{model}")
        } else {
            model.to_string()
        });
    }

    let provider = provider?;
    let new_body = serde_json::to_vec(&value).ok()?;
    Some((provider, new_body))
}

/// Strip provider prefix from a model in a URI path.
/// Returns `(provider, new_path)` if a prefix was found.
pub fn strip_provider_from_uri_path(path: &str) -> Option<(String, String)> {
    let model_in_path = extract_model_from_uri_path(path)?;
    let (has_models, provider, model) = split_provider_prefixed_model(&model_in_path)?;
    let provider = provider.to_string();
    let model = model.to_string();
    let old_segment = if has_models {
        format!("models/{model_in_path}")
    } else {
        model_in_path
    };
    let new_segment = if has_models {
        format!("models/{model}")
    } else {
        model
    };
    let new_path = path.replace(&old_segment, &new_segment);
    Some((provider, new_path))
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn body_model_pointers(op: OperationFamily, proto: ProtocolKind) -> &'static [&'static str] {
    match (op, proto) {
        (OperationFamily::ModelGet, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => &[],
        (
            OperationFamily::GenerateContent | OperationFamily::StreamGenerateContent,
            ProtocolKind::Gemini | ProtocolKind::GeminiNDJson,
        ) => &[],
        (OperationFamily::Embedding, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => &[],
        (OperationFamily::ModelGet, _) => &[],
        (OperationFamily::CountToken, ProtocolKind::Gemini | ProtocolKind::GeminiNDJson) => {
            &["/generate_content_request/model"]
        }
        (OperationFamily::GeminiLive, ProtocolKind::Gemini) => &["/setup/model"],
        _ => &["/model"],
    }
}
