use axum::body::Bytes;
use axum::http::StatusCode;
use serde::de::DeserializeOwned;

use super::{HttpError, bad_request};

pub(super) fn parse_json_body<T: DeserializeOwned>(
    body: &Bytes,
    context: &str,
) -> Result<T, HttpError> {
    serde_json::from_slice(body).map_err(|err| bad_request(format!("{context}: {err}")))
}

pub(super) fn split_provider_prefixed_plain_model(
    raw: &str,
) -> Result<(String, String), HttpError> {
    split_provider_prefixed_model(raw, false)
}

pub(super) fn split_provider_prefixed_model_path(raw: &str) -> Result<(String, String), HttpError> {
    split_provider_prefixed_model(raw, true)
}

fn split_provider_prefixed_model(
    raw: &str,
    allow_models_prefix: bool,
) -> Result<(String, String), HttpError> {
    let value = raw.trim().trim_matches('/');
    if value.is_empty() {
        return Err(bad_request("model is empty"));
    }

    let (has_models_prefix, tail) = if let Some(rest) = value.strip_prefix("models/") {
        if !allow_models_prefix {
            return Err(bad_request(
                "model prefix `models/` is not allowed for this endpoint",
            ));
        }
        (true, rest)
    } else {
        (false, value)
    };

    let (provider, model_without_provider) = tail
        .split_once('/')
        .ok_or_else(|| bad_request(format!("model must be prefixed as `<provider>/...`: {raw}")))?;

    if provider.trim().is_empty() || model_without_provider.trim().is_empty() {
        return Err(bad_request(format!(
            "invalid provider-prefixed model: {raw}"
        )));
    }

    let stripped = if has_models_prefix {
        format!("models/{model_without_provider}")
    } else {
        model_without_provider.to_string()
    };

    Ok((provider.to_string(), stripped))
}

pub(super) fn split_provider_prefixed_gemini_target(
    target: &str,
) -> Result<(String, String), HttpError> {
    for suffix in [
        ":generateContent",
        ":streamGenerateContent",
        ":countTokens",
        ":embedContent",
    ] {
        if let Some(model) = target.strip_suffix(suffix) {
            let (provider_name, stripped_model) = split_provider_prefixed_model_path(model)?;
            return Ok((provider_name, format!("{stripped_model}{suffix}")));
        }
    }
    Err(HttpError::new(
        StatusCode::NOT_FOUND,
        format!("unsupported gemini endpoint target: {target}"),
    ))
}

pub(super) fn normalize_gemini_model_path(value: &str) -> Result<String, HttpError> {
    let trimmed = value.trim_matches('/').trim();
    if trimmed.is_empty() {
        return Err(bad_request("gemini model path is empty"));
    }
    if trimmed.starts_with("models/") {
        Ok(trimmed.to_string())
    } else {
        Ok(format!("models/{trimmed}"))
    }
}

pub(super) fn normalize_unscoped_model_id(raw: &str) -> String {
    raw.trim()
        .trim_matches('/')
        .strip_prefix("models/")
        .unwrap_or(raw.trim().trim_matches('/'))
        .to_string()
}
