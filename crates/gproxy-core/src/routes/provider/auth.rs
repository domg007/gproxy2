use std::collections::BTreeMap;

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

const PASSTHROUGH_HEADER_DENYLIST: &[&str] = &[
    "host",
    "via",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "content-length",
    "sec-fetch-mode",
    "authorization",
    "x-api-key",
    "x-goog-api-key",
    "user-agent",
    "accept",
    "accept-encoding",
    "accept-language",
    "content-type",
    crate::INTERNAL_DOWNSTREAM_TRACE_ID_HEADER,
];

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
            forced_credential_id: None,
        });
    }

    Err(HttpError::from(gproxy_admin::AdminApiError::Unauthorized))
}

pub(super) fn resolve_provider(
    state: &AppState,
    provider_name: &str,
) -> Result<(ChannelId, ProviderDefinition), HttpError> {
    let channel = ChannelId::parse(provider_name);
    let snapshot = state.load_config();
    let Some(provider) = snapshot.providers.get(&channel).cloned() else {
        return Err(HttpError::new(
            StatusCode::NOT_FOUND,
            format!("provider not found: {provider_name}"),
        ));
    };
    Ok((channel, provider))
}

pub(super) fn restrict_provider_to_credential(
    mut provider: ProviderDefinition,
    credential_id: i64,
) -> Result<ProviderDefinition, HttpError> {
    let Some(credential) = provider.credentials.credential(credential_id).cloned() else {
        return Err(HttpError::new(
            StatusCode::NOT_FOUND,
            format!("credential not found: {credential_id}"),
        ));
    };
    provider.credentials.credentials = vec![credential];
    provider.credentials.channel_states.clear();
    Ok(provider)
}

pub(super) fn collect_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, _)| {
            !name
                .as_str()
                .eq_ignore_ascii_case(INTERNAL_DOWNSTREAM_TRACE_ID_HEADER)
        })
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn should_drop_passthrough_header(name: &str, websocket: bool) -> bool {
    if PASSTHROUGH_HEADER_DENYLIST
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
    {
        return true;
    }
    websocket
        && (name.eq_ignore_ascii_case("connection")
            || name.eq_ignore_ascii_case("upgrade")
            || name.to_ascii_lowercase().starts_with("sec-websocket-"))
}

fn collect_filtered_passthrough_headers(
    headers: &HeaderMap,
    websocket: bool,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (name, value) in headers {
        let name = name.as_str();
        if should_drop_passthrough_header(name, websocket) {
            continue;
        }
        let Ok(value) = value.to_str() else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        out.insert(name.to_string(), value.to_string());
    }
    out
}

pub(super) fn collect_passthrough_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    collect_filtered_passthrough_headers(headers, false)
}

pub(super) fn collect_websocket_passthrough_headers(
    headers: &HeaderMap,
) -> BTreeMap<String, String> {
    collect_filtered_passthrough_headers(headers, true)
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

#[cfg(test)]
mod tests {
    use super::{collect_passthrough_headers, collect_websocket_passthrough_headers};
    use axum::http::{HeaderMap, HeaderName, HeaderValue};

    fn headers(values: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in values {
            headers.insert(
                HeaderName::from_bytes(name.as_bytes()).expect("valid header name"),
                HeaderValue::from_str(value).expect("valid header value"),
            );
        }
        headers
    }

    #[test]
    fn passthrough_headers_drop_denylist_case_insensitively() {
        let headers = headers(&[
            ("Host", "example.com"),
            ("Authorization", "Bearer test"),
            ("USER-AGENT", "ua"),
            ("originator", "codex_vscode"),
            ("x-stainless-lang", "js"),
        ]);
        let filtered = collect_passthrough_headers(&headers);
        assert_eq!(
            filtered.get("originator").map(String::as_str),
            Some("codex_vscode")
        );
        assert_eq!(
            filtered.get("x-stainless-lang").map(String::as_str),
            Some("js")
        );
        assert!(!filtered.contains_key("Host"));
        assert!(!filtered.contains_key("Authorization"));
        assert!(!filtered.contains_key("USER-AGENT"));
    }

    #[test]
    fn passthrough_headers_skip_empty_values() {
        let headers = headers(&[("x-app", "cli"), ("x-empty", " "), ("x-space", "")]);
        let filtered = collect_passthrough_headers(&headers);
        assert_eq!(filtered.get("x-app").map(String::as_str), Some("cli"));
        assert!(!filtered.contains_key("x-empty"));
        assert!(!filtered.contains_key("x-space"));
    }

    #[test]
    fn websocket_passthrough_headers_drop_transport_headers() {
        let headers = headers(&[
            ("connection", "Upgrade"),
            ("upgrade", "websocket"),
            ("sec-websocket-key", "abc"),
            ("openai-beta", "responses_websockets=2026-02-04"),
            ("x-codex-turn-metadata", "turn-1"),
        ]);
        let filtered = collect_websocket_passthrough_headers(&headers);
        assert_eq!(
            filtered.get("openai-beta").map(String::as_str),
            Some("responses_websockets=2026-02-04")
        );
        assert_eq!(
            filtered.get("x-codex-turn-metadata").map(String::as_str),
            Some("turn-1")
        );
        assert!(!filtered.contains_key("connection"));
        assert!(!filtered.contains_key("upgrade"));
        assert!(!filtered.contains_key("sec-websocket-key"));
    }
}
