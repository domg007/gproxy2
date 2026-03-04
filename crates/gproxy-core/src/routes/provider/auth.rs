use axum::body::Body;
use axum::http::header::{HeaderName, HeaderValue};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use gproxy_protocol::claude::types::{AnthropicBeta, AnthropicVersion};
use gproxy_provider::{ChannelId, ProviderDefinition, parse_query_value};

use crate::AppState;
use crate::INTERNAL_DOWNSTREAM_TRACE_ID_HEADER;

use super::{
    AUTHORIZATION, CLAUDE_ANTHROPIC_BETA_HEADER, CLAUDE_ANTHROPIC_VERSION_HEADER, HttpError,
    RequestAuthContext, X_API_KEY, X_GOOG_API_KEY,
};

pub(super) fn header_value<'a>(headers: &'a HeaderMap, name: &'static str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

pub(super) async fn normalize_provider_auth_header(
    mut request: Request<Body>,
    next: Next,
) -> Response {
    if !request.headers().contains_key(X_API_KEY) {
        let candidate = header_value(request.headers(), X_GOOG_API_KEY)
            .map(ToString::to_string)
            .or_else(|| parse_query_value(request.uri().query(), "key"));
        if let Some(value) = candidate
            && let Ok(value) = HeaderValue::from_str(value.as_str())
        {
            request
                .headers_mut()
                .insert(HeaderName::from_static(X_API_KEY), value);
        }
    }
    next.run(request).await
}

fn extract_provider_api_key(headers: &HeaderMap) -> Option<&str> {
    if let Some(api_key) = header_value(headers, X_API_KEY) {
        return Some(api_key);
    }
    if let Some(api_key) = header_value(headers, X_GOOG_API_KEY) {
        return Some(api_key);
    }
    let authorization = header_value(headers, AUTHORIZATION)?;
    authorization
        .strip_prefix("Bearer ")
        .or_else(|| authorization.strip_prefix("bearer "))
}

fn parse_downstream_trace_id(headers: &HeaderMap) -> Option<i64> {
    header_value(headers, INTERNAL_DOWNSTREAM_TRACE_ID_HEADER)
        .and_then(|raw| raw.parse::<i64>().ok())
}

pub(super) fn authorize_provider_access(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<RequestAuthContext, HttpError> {
    let api_key = gproxy_admin::extract_api_key(extract_provider_api_key(headers))
        .map_err(HttpError::from)?;
    if let Some(key) = state.authenticate_api_key_in_memory(api_key) {
        return Ok(RequestAuthContext {
            user_id: key.user_id,
            user_key_id: key.id,
            downstream_trace_id: parse_downstream_trace_id(headers),
        });
    }

    Err(HttpError::from(gproxy_admin::AdminApiError::Unauthorized))
}

pub(super) fn resolve_provider(
    state: &AppState,
    provider_name: &str,
) -> Result<(ChannelId, ProviderDefinition), HttpError> {
    let channel = ChannelId::parse(provider_name);
    let snapshot = state.config.load();
    let Some(provider) = snapshot.providers.get(&channel).cloned() else {
        return Err(HttpError::new(
            StatusCode::NOT_FOUND,
            format!("provider not found: {provider_name}"),
        ));
    };
    Ok((channel, provider))
}

pub(super) fn collect_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, _)| !name.as_str().eq_ignore_ascii_case(INTERNAL_DOWNSTREAM_TRACE_ID_HEADER))
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn parse_anthropic_version(value: &str) -> AnthropicVersion {
    match value.trim() {
        "2023-01-01" => AnthropicVersion::V20230101,
        _ => AnthropicVersion::V20230601,
    }
}

fn parse_anthropic_beta(value: &str) -> AnthropicBeta {
    let trimmed = value.trim();
    let escaped = trimmed.replace('\\', "\\\\").replace('"', "\\\"");
    let payload = format!("\"{escaped}\"");
    serde_json::from_str::<AnthropicBeta>(&payload)
        .unwrap_or_else(|_| AnthropicBeta::Custom(trimmed.to_string()))
}

pub(super) fn anthropic_headers_from_request(
    headers: &HeaderMap,
) -> (AnthropicVersion, Option<Vec<AnthropicBeta>>) {
    let version = headers
        .get(CLAUDE_ANTHROPIC_VERSION_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(parse_anthropic_version)
        .unwrap_or_default();
    let betas = headers
        .get(CLAUDE_ANTHROPIC_BETA_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(parse_anthropic_beta)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty());
    (version, betas)
}
