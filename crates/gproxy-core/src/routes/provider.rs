use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::from_fn;
use axum::response::Response;
use axum::routing::{get, post};
use futures_util::Stream;
use gproxy_middleware::{
    MiddlewareTransformError, OperationFamily, ProtocolKind, TransformRequest,
    TransformRequestPayload, TransformResponsePayload, UsageSnapshot, attach_usage_extractor,
};
use gproxy_protocol::claude::count_tokens::request as claude_count_tokens_request;
use gproxy_protocol::claude::count_tokens::response as claude_count_tokens_response;
use gproxy_protocol::claude::create_message::response as claude_create_message_response;
use gproxy_protocol::claude::create_message::types::{BetaUsage, Model as ClaudeModel};
use gproxy_protocol::gemini::count_tokens::request as gemini_count_tokens_request;
use gproxy_protocol::gemini::count_tokens::response as gemini_count_tokens_response;
use gproxy_protocol::gemini::generate_content::response as gemini_generate_content_response;
use gproxy_protocol::gemini::generate_content::types::GeminiUsageMetadata;
use gproxy_protocol::openai::compact_response::response as openai_compact_response_response;
use gproxy_protocol::openai::compact_response::types::ResponseUsage as CompactResponseUsage;
use gproxy_protocol::openai::count_tokens::request as openai_count_tokens_request;
use gproxy_protocol::openai::count_tokens::response as openai_count_tokens_response;
use gproxy_protocol::openai::count_tokens::types::ResponseInput;
use gproxy_protocol::openai::create_chat_completions::response as openai_chat_completions_response;
use gproxy_protocol::openai::create_chat_completions::types::CompletionUsage;
use gproxy_protocol::openai::create_response::response as openai_create_response_response;
use gproxy_protocol::openai::create_response::types::ResponseUsage;
use gproxy_protocol::openai::embeddings::response as openai_embeddings_response;
use gproxy_protocol::openai::embeddings::types::OpenAiEmbeddingModel;
use gproxy_protocol::openai::embeddings::types::OpenAiEmbeddingUsage;
use gproxy_protocol::stream::SseToNdjsonRewriter;
use serde_json::json;
use tokio::sync::mpsc;

use gproxy_provider::{
    BuiltinChannel, ChannelId, ProviderDefinition, RetryWithPayloadRequest, RouteImplementation,
    RouteKey, TokenizerResolutionContext, TrackedHttpEvent, UpstreamError, UpstreamOAuthResponse,
    UpstreamRequestMeta, UpstreamResponse, capture_tracked_http_events, parse_query_value,
    try_local_response_for_channel,
};
use gproxy_storage::{CredentialStatusWrite, StorageWriteEvent, UpstreamRequestWrite, UsageWrite};

use crate::AppState;

use super::error::HttpError;

mod handlers;
use handlers::*;
mod auth;
use auth::*;
mod model_prefix;
use model_prefix::*;
mod recording;
use recording::*;
mod execute;
use execute::*;
mod context;
use context::*;
mod persistence;
use persistence::*;
mod catalog;
use catalog::*;

const X_API_KEY: &str = "x-api-key";
const X_GOOG_API_KEY: &str = "x-goog-api-key";
const AUTHORIZATION: &str = "authorization";
const CLAUDE_ANTHROPIC_VERSION_HEADER: &str = "anthropic-version";
const CLAUDE_ANTHROPIC_BETA_HEADER: &str = "anthropic-beta";
const BODY_CAPTURE_LIMIT_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModelProtocolPreference {
    OpenAi,
    Claude,
    Gemini,
}

pub(super) fn model_protocol_preference(
    headers: &HeaderMap,
    raw_query: Option<&str>,
) -> ModelProtocolPreference {
    let has_gemini_auth = has_gemini_model_auth(headers, raw_query);
    if has_gemini_auth {
        return ModelProtocolPreference::Gemini;
    }
    let has_bearer = header_value(headers, AUTHORIZATION).is_some();
    if has_bearer && headers.contains_key(CLAUDE_ANTHROPIC_VERSION_HEADER) {
        return ModelProtocolPreference::Claude;
    }
    if has_bearer {
        return ModelProtocolPreference::OpenAi;
    }
    ModelProtocolPreference::OpenAi
}

pub(super) fn has_gemini_model_auth(headers: &HeaderMap, raw_query: Option<&str>) -> bool {
    headers.contains_key(X_GOOG_API_KEY) || parse_query_value(raw_query, "key").is_some()
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/messages", post(claude_messages_unscoped))
        .route(
            "/v1/messages/count_tokens",
            post(claude_count_tokens_unscoped),
        )
        .route(
            "/v1/chat/completions",
            post(openai_chat_completions_unscoped),
        )
        .route(
            "/v1/responses",
            post(openai_responses_unscoped).get(openai_responses_upgrade_unscoped),
        )
        .route("/v1/images/generations", post(openai_create_image_unscoped))
        .route("/v1/images/edits", post(openai_create_image_edit_unscoped))
        .route(
            "/v1/responses/input_tokens",
            post(openai_input_tokens_unscoped),
        )
        .route("/v1/embeddings", post(openai_embeddings_unscoped))
        .route("/v1/responses/compact", post(openai_compact_unscoped))
        .route("/v1/models", get(v1_model_list_unscoped))
        .route("/v1/models/{*model_id}", get(v1_model_get_unscoped))
        .route("/v1beta/models", get(v1beta_model_list_unscoped))
        .route("/v1beta/{*target}", get(v1beta_model_get_unscoped))
        .route("/v1beta/{*target}", post(v1beta_post_target_unscoped))
        .route("/v1/{*target}", post(v1_post_target_unscoped))
        .route("/{provider}/v1/oauth", get(oauth_start))
        .route("/{provider}/v1/oauth/callback", get(oauth_callback))
        .route("/{provider}/v1/usage", get(upstream_usage))
        .route("/{provider}/v1/realtime", get(openai_realtime_upgrade))
        .route(
            "/{provider}/v1/realtime/{*tail}",
            get(openai_realtime_upgrade_with_tail),
        )
        .route("/{provider}/v1/messages", post(claude_messages))
        .route(
            "/{provider}/v1/messages/count_tokens",
            post(claude_count_tokens),
        )
        .route(
            "/{provider}/v1/chat/completions",
            post(openai_chat_completions),
        )
        .route(
            "/{provider}/v1/responses",
            post(openai_responses).get(openai_responses_upgrade),
        )
        .route(
            "/{provider}/v1/images/generations",
            post(openai_create_image),
        )
        .route(
            "/{provider}/v1/images/edits",
            post(openai_create_image_edit),
        )
        .route(
            "/{provider}/v1/responses/input_tokens",
            post(openai_input_tokens),
        )
        .route("/{provider}/v1/embeddings", post(openai_embeddings))
        .route("/{provider}/v1/responses/compact", post(openai_compact))
        .route("/{provider}/v1/models", get(v1_model_list))
        .route("/{provider}/v1/models/{*model_id}", get(v1_model_get))
        .route("/{provider}/v1beta/models", get(v1beta_model_list))
        .route("/{provider}/v1beta/{*target}", get(v1beta_model_get))
        .route("/{provider}/v1beta/{*target}", post(v1beta_post_target))
        .route("/{provider}/v1/{*target}", post(v1_post_target))
        .layer(from_fn(normalize_provider_auth_header))
}

fn oauth_response_to_axum(response: UpstreamOAuthResponse) -> Response {
    let status = StatusCode::from_u16(response.status_code).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    for (name, value) in response.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder
        .body(Body::from(response.body))
        .unwrap_or_else(|_| Response::new(Body::from("failed to build provider response")))
}

fn oauth_callback_response_to_axum(
    result: gproxy_provider::UpstreamOAuthCallbackResult,
    credential_id: Option<i64>,
) -> Response {
    let upstream = serde_json::from_slice::<serde_json::Value>(&result.response.body)
        .unwrap_or_else(|_| {
            serde_json::Value::String(String::from_utf8_lossy(&result.response.body).to_string())
        });
    let body = serde_json::to_vec(&json!({
        "upstream": upstream,
        "credential": result.credential,
        "credential_id": credential_id,
    }))
    .unwrap_or_default();

    let mut headers = result.response.headers;
    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("content-type"));
    headers.push(("content-type".to_string(), "application/json".to_string()));

    oauth_response_to_axum(UpstreamOAuthResponse {
        status_code: result.response.status_code,
        headers,
        body,
        request_meta: result.response.request_meta,
    })
}

fn bad_request(message: impl Into<String>) -> HttpError {
    HttpError::new(StatusCode::BAD_REQUEST, message)
}

fn internal_error(message: impl Into<String>) -> HttpError {
    HttpError::new(StatusCode::INTERNAL_SERVER_ERROR, message)
}

fn parse_optional_query_value<T>(query: Option<&str>, key: &str) -> Result<Option<T>, HttpError>
where
    T: FromStr,
{
    let Some(raw) = parse_query_value(query, key) else {
        return Ok(None);
    };
    raw.parse::<T>()
        .map(Some)
        .map_err(|_| bad_request(format!("invalid query parameter `{key}`: {raw}")))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderName, HeaderValue};

    use super::{ModelProtocolPreference, model_protocol_preference};

    fn headers(values: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in values {
            let header_name = HeaderName::from_bytes(name.as_bytes()).expect("valid header name");
            headers.insert(
                header_name,
                HeaderValue::from_str(value).expect("valid header value"),
            );
        }
        headers
    }

    #[test]
    fn model_list_prefers_openai_for_x_api_key_without_bearer() {
        let headers = headers(&[("x-api-key", "test")]);
        assert_eq!(
            model_protocol_preference(&headers, None),
            ModelProtocolPreference::OpenAi
        );
    }

    #[test]
    fn model_list_prefers_openai_for_anthropic_version_without_bearer() {
        let headers = headers(&[("anthropic-version", "2023-06-01")]);
        assert_eq!(
            model_protocol_preference(&headers, None),
            ModelProtocolPreference::OpenAi
        );
    }

    #[test]
    fn model_list_prefers_claude_for_anthropic_version_even_with_bearer() {
        let headers = headers(&[
            ("anthropic-version", "2023-06-01"),
            ("authorization", "Bearer test"),
        ]);
        assert_eq!(
            model_protocol_preference(&headers, None),
            ModelProtocolPreference::Claude
        );
    }

    #[test]
    fn model_list_prefers_gemini_for_query_key() {
        let headers = HeaderMap::new();
        assert_eq!(
            model_protocol_preference(&headers, Some("key=test")),
            ModelProtocolPreference::Gemini
        );
    }

    #[test]
    fn model_list_prefers_gemini_for_x_goog_api_key() {
        let headers = headers(&[("x-goog-api-key", "test")]);
        assert_eq!(
            model_protocol_preference(&headers, None),
            ModelProtocolPreference::Gemini
        );
    }

    #[test]
    fn model_list_uses_openai_for_bearer_by_default() {
        let headers = headers(&[("authorization", "Bearer test")]);
        assert_eq!(
            model_protocol_preference(&headers, None),
            ModelProtocolPreference::OpenAi
        );
    }
}
