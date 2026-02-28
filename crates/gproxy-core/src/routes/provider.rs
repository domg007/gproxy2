use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{Path, RawQuery, State};
use axum::http::header::{HeaderName, HeaderValue};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::middleware::{Next, from_fn};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::Stream;
use futures_util::StreamExt;
use gproxy_middleware::{
    MiddlewareTransformError, OperationFamily, ProtocolKind, TransformRequest,
    TransformResponsePayload, UsageSnapshot, attach_usage_extractor,
};
use gproxy_protocol::claude::count_tokens::request as claude_count_tokens_request;
use gproxy_protocol::claude::count_tokens::response as claude_count_tokens_response;
use gproxy_protocol::claude::create_message::request as claude_create_message_request;
use gproxy_protocol::claude::create_message::response as claude_create_message_response;
use gproxy_protocol::claude::create_message::types::{BetaUsage, Model as ClaudeModel};
use gproxy_protocol::claude::model_get::request as claude_model_get_request;
use gproxy_protocol::claude::model_list::request as claude_model_list_request;
use gproxy_protocol::claude::types::{AnthropicBeta, AnthropicVersion};
use gproxy_protocol::gemini::count_tokens::request as gemini_count_tokens_request;
use gproxy_protocol::gemini::count_tokens::response as gemini_count_tokens_response;
use gproxy_protocol::gemini::embeddings::request as gemini_embeddings_request;
use gproxy_protocol::gemini::generate_content::request as gemini_generate_content_request;
use gproxy_protocol::gemini::generate_content::response as gemini_generate_content_response;
use gproxy_protocol::gemini::generate_content::types::GeminiUsageMetadata;
use gproxy_protocol::gemini::model_get::request as gemini_model_get_request;
use gproxy_protocol::gemini::model_list::request as gemini_model_list_request;
use gproxy_protocol::gemini::stream_generate_content::request as gemini_stream_generate_content_request;
use gproxy_protocol::openai::compact_response::request as openai_compact_request;
use gproxy_protocol::openai::compact_response::response as openai_compact_response_response;
use gproxy_protocol::openai::compact_response::types::ResponseUsage as CompactResponseUsage;
use gproxy_protocol::openai::count_tokens::request as openai_count_tokens_request;
use gproxy_protocol::openai::count_tokens::response as openai_count_tokens_response;
use gproxy_protocol::openai::count_tokens::types::ResponseInput;
use gproxy_protocol::openai::create_chat_completions::request as openai_chat_completions_request;
use gproxy_protocol::openai::create_chat_completions::response as openai_chat_completions_response;
use gproxy_protocol::openai::create_chat_completions::types::CompletionUsage;
use gproxy_protocol::openai::create_response::request as openai_create_response_request;
use gproxy_protocol::openai::create_response::response as openai_create_response_response;
use gproxy_protocol::openai::create_response::types::ResponseUsage;
use gproxy_protocol::openai::embeddings::request as openai_embeddings_request;
use gproxy_protocol::openai::embeddings::response as openai_embeddings_response;
use gproxy_protocol::openai::embeddings::types::OpenAiEmbeddingModel;
use gproxy_protocol::openai::embeddings::types::OpenAiEmbeddingUsage;
use gproxy_protocol::openai::model_get::request as openai_model_get_request;
use gproxy_protocol::openai::model_list::request as openai_model_list_request;
use gproxy_protocol::stream::SseToNdjsonRewriter;
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio::sync::mpsc;

use gproxy_provider::{
    BuiltinChannel, BuiltinChannelCredential, ChannelCredential, ChannelId, CredentialHealth,
    CredentialRef, ProviderDefinition, RouteImplementation, RouteKey, TokenizerResolutionContext,
    UpstreamCredentialUpdate, UpstreamError, UpstreamOAuthRequest, UpstreamOAuthResponse,
    UpstreamRequestMeta, UpstreamResponse, normalize_antigravity_upstream_response_body,
    normalize_antigravity_upstream_stream_ndjson_chunk, normalize_geminicli_upstream_response_body,
    normalize_geminicli_upstream_stream_ndjson_chunk, normalize_vertex_upstream_response_body,
    parse_query_value, try_local_vertexexpress_model_response,
};
use gproxy_storage::{
    CredentialQuery, CredentialStatusWrite, CredentialWrite, ProviderQuery, ProviderWrite, Scope,
    StorageWriteBatch, StorageWriteEvent, StorageWriteSink, UpstreamRequestWrite, UsageWrite,
};

use crate::AppState;

use super::error::HttpError;

const X_API_KEY: &str = "x-api-key";
const X_GOOG_API_KEY: &str = "x-goog-api-key";
const AUTHORIZATION: &str = "authorization";
const CLAUDE_ANTHROPIC_VERSION_HEADER: &str = "anthropic-version";
const CLAUDE_ANTHROPIC_BETA_HEADER: &str = "anthropic-beta";
const BODY_CAPTURE_LIMIT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct RequestAuthContext {
    user_id: i64,
    user_key_id: i64,
}

#[derive(Clone)]
struct UpstreamStreamRecordContext {
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    request: TransformRequest,
    provider_id: Option<i64>,
    credential_id: Option<i64>,
    request_meta: Option<UpstreamRequestMeta>,
    response_status: Option<u16>,
    response_headers: Vec<(String, String)>,
    stream_usage: Option<gproxy_middleware::UsageHandle>,
}

#[derive(Default)]
struct UpstreamStreamRecordState {
    captured: Vec<u8>,
    capture_truncated: bool,
    flushed: bool,
}

struct UpstreamStreamRecordGuard {
    context: UpstreamStreamRecordContext,
    state: Arc<Mutex<UpstreamStreamRecordState>>,
}

impl UpstreamStreamRecordGuard {
    fn new(context: UpstreamStreamRecordContext) -> Self {
        Self {
            context,
            state: Arc::new(Mutex::new(UpstreamStreamRecordState::default())),
        }
    }

    fn push_chunk(&self, chunk: &[u8]) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.capture_truncated {
            return;
        }
        let remaining = BODY_CAPTURE_LIMIT_BYTES.saturating_sub(state.captured.len());
        if remaining > 0 {
            let take = chunk.len().min(remaining);
            state.captured.extend_from_slice(&chunk[..take]);
        }
        if state.captured.len() >= BODY_CAPTURE_LIMIT_BYTES {
            state.capture_truncated = true;
        }
    }

    fn take_flush_payload(&self) -> Option<(UpstreamStreamRecordContext, Option<Vec<u8>>)> {
        let Ok(mut state) = self.state.lock() else {
            return None;
        };
        if state.flushed {
            return None;
        }
        state.flushed = true;
        let response_body =
            (!state.captured.is_empty()).then(|| std::mem::take(&mut state.captured));
        Some((self.context.clone(), response_body))
    }

    async fn flush_now(&self) {
        if let Some((context, response_body)) = self.take_flush_payload() {
            let response_body_for_usage = response_body.clone();
            let stream_usage = context
                .stream_usage
                .as_ref()
                .and_then(|handle| handle.latest());
            enqueue_upstream_request_event_from_meta(
                context.state.as_ref(),
                context.provider_id,
                context.credential_id,
                context.request_meta.as_ref(),
                context.response_status,
                context.response_headers.as_slice(),
                response_body,
            )
            .await;
            enqueue_stream_usage_event_with_estimate(
                &context,
                response_body_for_usage.as_deref().unwrap_or(&[]),
                stream_usage,
            )
            .await;
        }
    }
}

impl Drop for UpstreamStreamRecordGuard {
    fn drop(&mut self) {
        let Some((context, response_body)) = self.take_flush_payload() else {
            return;
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async move {
            let response_body_for_usage = response_body.clone();
            let stream_usage = context
                .stream_usage
                .as_ref()
                .and_then(|handle| handle.latest());
            enqueue_upstream_request_event_from_meta(
                context.state.as_ref(),
                context.provider_id,
                context.credential_id,
                context.request_meta.as_ref(),
                context.response_status,
                context.response_headers.as_slice(),
                response_body,
            )
            .await;
            enqueue_stream_usage_event_with_estimate(
                &context,
                response_body_for_usage.as_deref().unwrap_or(&[]),
                stream_usage,
            )
            .await;
        });
    }
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

fn header_value<'a>(headers: &'a HeaderMap, name: &'static str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

async fn normalize_provider_auth_header(mut request: Request<Body>, next: Next) -> Response {
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

fn authorize_provider_access(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<RequestAuthContext, HttpError> {
    let api_key = gproxy_admin::extract_api_key(extract_provider_api_key(headers))
        .map_err(HttpError::from)?;
    if let Some(key) = state.authenticate_api_key_in_memory(api_key) {
        return Ok(RequestAuthContext {
            user_id: key.user_id,
            user_key_id: key.id,
        });
    }

    Err(HttpError::from(gproxy_admin::AdminApiError::Unauthorized))
}

fn resolve_provider(
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

fn collect_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn now_unix_ms_i64() -> i64 {
    i64::try_from(now_unix_ms()).unwrap_or(i64::MAX)
}

fn headers_pairs_to_json(headers: &[(String, String)]) -> String {
    let mut map: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (name, value) in headers {
        map.entry(name.clone()).or_default().push(value.clone());
    }
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
}

fn response_headers_to_pairs(response: &wreq::Response) -> Vec<(String, String)> {
    response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn should_record_usage(operation: OperationFamily) -> bool {
    !matches!(
        operation,
        OperationFamily::ModelList | OperationFamily::ModelGet
    )
}

fn upstream_error_request_meta(error: &UpstreamError) -> Option<UpstreamRequestMeta> {
    match error {
        UpstreamError::AllCredentialsExhausted {
            last_request_meta, ..
        } => last_request_meta.as_deref().cloned(),
        _ => None,
    }
}

fn upstream_error_credential_id(error: &UpstreamError) -> Option<i64> {
    match error {
        UpstreamError::AllCredentialsExhausted {
            last_credential_id, ..
        } => *last_credential_id,
        _ => None,
    }
}

fn upstream_error_status(error: &UpstreamError) -> Option<u16> {
    match error {
        UpstreamError::AllCredentialsExhausted { last_status, .. } => *last_status,
        _ => None,
    }
}

fn credential_health_to_storage(health: &CredentialHealth) -> (String, Option<String>) {
    match health {
        CredentialHealth::Healthy => ("healthy".to_string(), None),
        CredentialHealth::Dead => ("dead".to_string(), None),
        CredentialHealth::Partial { models } => {
            ("partial".to_string(), serde_json::to_string(models).ok())
        }
    }
}

async fn enqueue_credential_status_updates_for_request(
    state: &AppState,
    channel: &ChannelId,
    provider: &ProviderDefinition,
    request_now_unix_ms: u64,
) {
    for credential in provider.credentials.list_credentials() {
        let Some(state_row) = state.credential_states.get(channel, credential.id) else {
            continue;
        };
        if state_row.checked_at_unix_ms != Some(request_now_unix_ms) {
            continue;
        }

        let (health_kind, health_json) = credential_health_to_storage(&state_row.health);
        let checked_at_unix_ms = state_row
            .checked_at_unix_ms
            .and_then(|value| i64::try_from(value).ok());
        let event = StorageWriteEvent::UpsertCredentialStatus(CredentialStatusWrite {
            id: None,
            credential_id: credential.id,
            channel: channel.as_str().to_string(),
            health_kind,
            health_json,
            checked_at_unix_ms,
            last_error: state_row.last_error.clone(),
        });
        if let Err(err) = state.enqueue_storage_write(event).await {
            eprintln!(
                "provider: credential status enqueue failed channel={} credential_id={} error={}",
                channel.as_str(),
                credential.id,
                err
            );
        }
    }
}

fn extract_local_count_input_tokens(
    response: &gproxy_middleware::TransformResponse,
) -> Option<i64> {
    match response {
        gproxy_middleware::TransformResponse::CountTokenOpenAi(
            openai_count_tokens_response::OpenAiCountTokensResponse::Success { body, .. },
        ) => i64::try_from(body.input_tokens).ok(),
        gproxy_middleware::TransformResponse::CountTokenClaude(
            claude_count_tokens_response::ClaudeCountTokensResponse::Success { body, .. },
        ) => i64::try_from(body.input_tokens).ok(),
        gproxy_middleware::TransformResponse::CountTokenGemini(
            gemini_count_tokens_response::GeminiCountTokensResponse::Success { body, .. },
        ) => i64::try_from(body.total_tokens).ok(),
        _ => None,
    }
}

fn extract_count_tokens_from_raw_json(protocol: ProtocolKind, body: &[u8]) -> Option<i64> {
    match protocol {
        ProtocolKind::OpenAi | ProtocolKind::OpenAiChatCompletion => {
            if let Ok(value) =
                serde_json::from_slice::<openai_count_tokens_response::ResponseBody>(body)
            {
                return i64::try_from(value.input_tokens).ok();
            }
            serde_json::from_slice::<openai_count_tokens_response::OpenAiCountTokensResponse>(body)
                .ok()
                .and_then(|value| match value {
                    openai_count_tokens_response::OpenAiCountTokensResponse::Success {
                        body,
                        ..
                    } => i64::try_from(body.input_tokens).ok(),
                    _ => None,
                })
        }
        ProtocolKind::Claude => {
            if let Ok(value) =
                serde_json::from_slice::<claude_count_tokens_response::ResponseBody>(body)
            {
                return i64::try_from(value.input_tokens).ok();
            }
            serde_json::from_slice::<claude_count_tokens_response::ClaudeCountTokensResponse>(body)
                .ok()
                .and_then(|value| match value {
                    claude_count_tokens_response::ClaudeCountTokensResponse::Success {
                        body,
                        ..
                    } => i64::try_from(body.input_tokens).ok(),
                    _ => None,
                })
        }
        ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
            if let Ok(value) =
                serde_json::from_slice::<gemini_count_tokens_response::ResponseBody>(body)
            {
                return i64::try_from(value.total_tokens).ok();
            }
            serde_json::from_slice::<gemini_count_tokens_response::GeminiCountTokensResponse>(body)
                .ok()
                .and_then(|value| match value {
                    gemini_count_tokens_response::GeminiCountTokensResponse::Success {
                        body,
                        ..
                    } => i64::try_from(body.total_tokens).ok(),
                    _ => None,
                })
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct UsageMetrics {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cache_read_input_tokens: Option<i64>,
    cache_creation_input_tokens: Option<i64>,
}

fn u64_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn usage_metrics_from_openai_response_usage(usage: &ResponseUsage) -> UsageMetrics {
    UsageMetrics {
        input_tokens: Some(u64_to_i64(usage.input_tokens)),
        output_tokens: Some(u64_to_i64(usage.output_tokens)),
        cache_read_input_tokens: Some(u64_to_i64(usage.input_tokens_details.cached_tokens)),
        cache_creation_input_tokens: None,
    }
}

fn usage_metrics_from_openai_compact_usage(usage: &CompactResponseUsage) -> UsageMetrics {
    UsageMetrics {
        input_tokens: Some(u64_to_i64(usage.input_tokens)),
        output_tokens: Some(u64_to_i64(usage.output_tokens)),
        cache_read_input_tokens: Some(u64_to_i64(usage.input_tokens_details.cached_tokens)),
        cache_creation_input_tokens: None,
    }
}

fn usage_metrics_from_openai_chat_completion_usage(usage: &CompletionUsage) -> UsageMetrics {
    UsageMetrics {
        input_tokens: Some(u64_to_i64(usage.prompt_tokens)),
        output_tokens: Some(u64_to_i64(usage.completion_tokens)),
        cache_read_input_tokens: usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|value| value.cached_tokens)
            .map(u64_to_i64),
        cache_creation_input_tokens: None,
    }
}

fn usage_metrics_from_claude_usage(usage: &BetaUsage) -> UsageMetrics {
    let input_tokens = usage
        .input_tokens
        .saturating_add(usage.cache_creation_input_tokens)
        .saturating_add(usage.cache_read_input_tokens);
    UsageMetrics {
        input_tokens: Some(u64_to_i64(input_tokens)),
        output_tokens: Some(u64_to_i64(usage.output_tokens)),
        cache_read_input_tokens: Some(u64_to_i64(usage.cache_read_input_tokens)),
        cache_creation_input_tokens: Some(u64_to_i64(usage.cache_creation_input_tokens)),
    }
}

fn usage_metrics_from_gemini_usage(usage: &GeminiUsageMetadata) -> UsageMetrics {
    let input_tokens = usage
        .prompt_token_count
        .unwrap_or(0)
        .saturating_add(usage.cached_content_token_count.unwrap_or(0));
    UsageMetrics {
        input_tokens: usage
            .prompt_token_count
            .or(usage.cached_content_token_count)
            .map(|_| u64_to_i64(input_tokens)),
        output_tokens: usage.candidates_token_count.map(u64_to_i64),
        cache_read_input_tokens: usage.cached_content_token_count.map(u64_to_i64),
        cache_creation_input_tokens: None,
    }
}

fn usage_metrics_from_openai_embeddings_usage(usage: &OpenAiEmbeddingUsage) -> UsageMetrics {
    let prompt_tokens = u64_to_i64(usage.prompt_tokens);
    let total_tokens = u64_to_i64(usage.total_tokens);
    UsageMetrics {
        input_tokens: Some(prompt_tokens),
        output_tokens: Some(total_tokens.saturating_sub(prompt_tokens)),
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
    }
}

fn extract_usage_from_local_response(
    response: &gproxy_middleware::TransformResponse,
) -> Option<UsageMetrics> {
    match response {
        gproxy_middleware::TransformResponse::CountTokenOpenAi(
            openai_count_tokens_response::OpenAiCountTokensResponse::Success { body, .. },
        ) => Some(UsageMetrics {
            input_tokens: Some(u64_to_i64(body.input_tokens)),
            output_tokens: Some(0),
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
        }),
        gproxy_middleware::TransformResponse::CountTokenClaude(
            claude_count_tokens_response::ClaudeCountTokensResponse::Success { body, .. },
        ) => Some(UsageMetrics {
            input_tokens: Some(u64_to_i64(body.input_tokens)),
            output_tokens: Some(0),
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
        }),
        gproxy_middleware::TransformResponse::CountTokenGemini(
            gemini_count_tokens_response::GeminiCountTokensResponse::Success { body, .. },
        ) => Some(UsageMetrics {
            input_tokens: Some(u64_to_i64(body.total_tokens)),
            output_tokens: Some(0),
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
        }),
        gproxy_middleware::TransformResponse::GenerateContentOpenAiResponse(
            openai_create_response_response::OpenAiCreateResponseResponse::Success { body, .. },
        ) => body
            .usage
            .as_ref()
            .map(usage_metrics_from_openai_response_usage),
        gproxy_middleware::TransformResponse::GenerateContentOpenAiChatCompletions(
            openai_chat_completions_response::OpenAiChatCompletionsResponse::Success {
                body, ..
            },
        ) => body
            .usage
            .as_ref()
            .map(usage_metrics_from_openai_chat_completion_usage),
        gproxy_middleware::TransformResponse::GenerateContentClaude(
            claude_create_message_response::ClaudeCreateMessageResponse::Success { body, .. },
        ) => Some(usage_metrics_from_claude_usage(&body.usage)),
        gproxy_middleware::TransformResponse::GenerateContentGemini(
            gemini_generate_content_response::GeminiGenerateContentResponse::Success {
                body, ..
            },
        ) => body
            .usage_metadata
            .as_ref()
            .map(usage_metrics_from_gemini_usage),
        gproxy_middleware::TransformResponse::EmbeddingOpenAi(
            openai_embeddings_response::OpenAiEmbeddingsResponse::Success { body, .. },
        ) => Some(usage_metrics_from_openai_embeddings_usage(&body.usage)),
        gproxy_middleware::TransformResponse::CompactOpenAi(
            openai_compact_response_response::OpenAiCompactResponse::Success { body, .. },
        ) => Some(usage_metrics_from_openai_compact_usage(&body.usage)),
        _ => None,
    }
}

fn decode_response_for_usage(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Option<gproxy_middleware::TransformResponse> {
    gproxy_middleware::decode_response_payload(operation, protocol, body).ok()
}

fn normalize_upstream_response_body_for_channel(
    channel: &ChannelId,
    body: &[u8],
) -> Option<Vec<u8>> {
    match channel {
        ChannelId::Builtin(BuiltinChannel::GeminiCli) => {
            normalize_geminicli_upstream_response_body(body)
        }
        ChannelId::Builtin(BuiltinChannel::Antigravity) => {
            normalize_antigravity_upstream_response_body(body)
        }
        ChannelId::Builtin(BuiltinChannel::Vertex) => normalize_vertex_upstream_response_body(body),
        _ => None,
    }
}

fn normalize_upstream_stream_ndjson_chunk_for_channel(
    channel: &ChannelId,
    chunk: &[u8],
) -> Option<Vec<u8>> {
    match channel {
        ChannelId::Builtin(BuiltinChannel::GeminiCli) => {
            normalize_geminicli_upstream_stream_ndjson_chunk(chunk)
        }
        ChannelId::Builtin(BuiltinChannel::Antigravity) => {
            normalize_antigravity_upstream_stream_ndjson_chunk(chunk)
        }
        _ => None,
    }
}

fn is_wrapped_stream_channel(channel: &ChannelId) -> bool {
    matches!(
        channel,
        ChannelId::Builtin(BuiltinChannel::GeminiCli)
            | ChannelId::Builtin(BuiltinChannel::Antigravity)
    )
}

fn ndjson_chunk_to_sse_chunk(chunk: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(chunk) else {
        return chunk.to_vec();
    };
    let mut out = String::with_capacity(text.len().saturating_mul(2));
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        out.push_str("data: ");
        out.push_str(line);
        out.push_str("\n\n");
    }
    out.into_bytes()
}

fn strip_model_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.retain(|key, _| !key.eq_ignore_ascii_case("model"));
            for child in object.values_mut() {
                strip_model_fields(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_model_fields(item);
            }
        }
        _ => {}
    }
}

async fn estimate_embedding_input_tokens_from_request(
    state: &AppState,
    request: &TransformRequest,
) -> Option<i64> {
    if request.operation() != OperationFamily::Embedding {
        return None;
    }
    let model = extract_model_from_request(request)?.trim().to_string();
    if model.is_empty() {
        return None;
    }
    let mut value = serde_json::to_value(request).ok()?;
    strip_model_fields(&mut value);
    let text = serde_json::to_string(&value).ok()?;
    let count = state
        .count_tokens_with_local_tokenizer(model.as_str(), text.as_str())
        .await
        .ok()?
        .count;
    i64::try_from(count).ok()
}

fn build_openai_count_request(
    model: &str,
    text: &str,
) -> openai_count_tokens_request::OpenAiCountTokensRequest {
    let mut request = openai_count_tokens_request::OpenAiCountTokensRequest::default();
    request.body.model = Some(model.to_string());
    request.body.input = Some(ResponseInput::Text(text.to_string()));
    request
}

fn normalize_count_source_text(source: &str) -> String {
    if source.trim().is_empty() {
        " ".to_string()
    } else {
        source.to_string()
    }
}

async fn estimate_tokens_with_channel_count(
    context: &UpstreamStreamRecordContext,
    model: &str,
    text: &str,
) -> Option<i64> {
    let source = normalize_count_source_text(text);
    let openai_request = build_openai_count_request(model, source.as_str());
    let mut candidates = vec![(
        ProtocolKind::OpenAi,
        TransformRequest::CountTokenOpenAi(openai_request.clone()),
    )];
    if let Ok(request) =
        claude_count_tokens_request::ClaudeCountTokensRequest::try_from(openai_request.clone())
    {
        candidates.push((
            ProtocolKind::Claude,
            TransformRequest::CountTokenClaude(request),
        ));
    }
    if let Ok(request) =
        gemini_count_tokens_request::GeminiCountTokensRequest::try_from(openai_request)
    {
        candidates.push((
            ProtocolKind::Gemini,
            TransformRequest::CountTokenGemini(request),
        ));
    }

    for (source_protocol, source_request) in candidates {
        let source_route = RouteKey::new(OperationFamily::CountToken, source_protocol);
        let Some(implementation) = context.provider.dispatch.resolve(source_route).cloned() else {
            continue;
        };
        let mut upstream_request = source_request.clone();
        let mut upstream_protocol = source_protocol;
        let execute_local = match implementation {
            RouteImplementation::Unsupported => continue,
            RouteImplementation::Local => true,
            RouteImplementation::Passthrough => false,
            RouteImplementation::TransformTo { destination } => {
                let route = gproxy_middleware::TransformRoute {
                    src_operation: source_route.operation,
                    src_protocol: source_route.protocol,
                    dst_operation: destination.operation,
                    dst_protocol: destination.protocol,
                };
                if !route.is_passthrough() {
                    let Ok(transformed) =
                        gproxy_middleware::transform_request(upstream_request.clone(), route)
                    else {
                        continue;
                    };
                    upstream_request = transformed;
                }
                upstream_protocol = destination.protocol;
                false
            }
        };

        if execute_local {
            let Ok(local) =
                execute_local_count_token_request(context.state.as_ref(), &source_request).await
            else {
                continue;
            };
            let Some(local_response) = local.local_response.as_ref() else {
                continue;
            };
            if let Some(tokens) = extract_local_count_input_tokens(local_response) {
                return Some(tokens);
            }
            continue;
        }

        let now = now_unix_ms();
        let http = if matches!(
            &context.channel,
            ChannelId::Builtin(BuiltinChannel::ClaudeCode)
        ) {
            context.state.load_spoof_http()
        } else {
            context.state.load_http()
        };
        let tokenizers = context.state.tokenizers();
        let global = context.state.config.load().global.clone();
        let Ok(upstream) = context
            .provider
            .execute_with_retry(
                http.as_ref(),
                &context.state.credential_states,
                &upstream_request,
                now,
                TokenizerResolutionContext {
                    tokenizer_store: tokenizers.as_ref(),
                    hf_token: global.hf_token.as_deref(),
                    hf_url: global.hf_url.as_deref(),
                },
            )
            .await
        else {
            continue;
        };

        if let Some(local_response) = upstream.local_response.as_ref()
            && let Some(tokens) = extract_local_count_input_tokens(local_response)
        {
            return Some(tokens);
        }

        let Some(response) = upstream.response else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(bytes) = response.bytes().await else {
            continue;
        };
        if let Some(tokens) = extract_count_tokens_from_raw_json(upstream_protocol, bytes.as_ref())
        {
            return Some(tokens);
        }
    }

    None
}

async fn estimate_tokens_for_text(
    context: &UpstreamStreamRecordContext,
    model: &str,
    text: &str,
) -> i64 {
    if let Some(tokens) = estimate_tokens_with_channel_count(context, model, text).await {
        return tokens.max(0);
    }
    context
        .state
        .count_tokens_with_local_tokenizer(model, text)
        .await
        .map(|count| i64::try_from(count.count).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

async fn enqueue_stream_usage_event_with_estimate(
    context: &UpstreamStreamRecordContext,
    stream_response_body: &[u8],
    stream_usage: Option<UsageSnapshot>,
) {
    if !should_record_usage(context.request.operation())
        || context
            .response_status
            .map(|status| status >= 400)
            .unwrap_or(true)
    {
        return;
    }

    let request_model = normalize_usage_model(extract_model_from_request(&context.request));
    let model = request_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("deepseek_fallback")
        .to_string();

    let usage = stream_usage.as_ref();
    let mut input_tokens = usage.and_then(|value| value.input_tokens).map(u64_to_i64);
    let mut output_tokens = usage.and_then(|value| value.output_tokens).map(u64_to_i64);
    let cache_read_input_tokens = usage
        .and_then(|value| value.cache_read_input_tokens)
        .map(u64_to_i64);
    let cache_creation_input_tokens = usage
        .and_then(|value| value.cache_creation_input_tokens)
        .map(u64_to_i64);

    if let Some(total) = usage.and_then(|value| value.total_tokens).map(u64_to_i64) {
        match (input_tokens, output_tokens) {
            (None, Some(output)) => {
                input_tokens = Some(total.saturating_sub(output));
            }
            (Some(input), None) => {
                output_tokens = Some(total.saturating_sub(input));
            }
            _ => {}
        }
    }

    if input_tokens.is_none()
        && output_tokens.is_none()
        && cache_read_input_tokens.is_none()
        && cache_creation_input_tokens.is_none()
    {
        let request_text = serde_json::to_string(&context.request).unwrap_or_default();
        let response_text = String::from_utf8_lossy(stream_response_body).to_string();

        input_tokens =
            Some(estimate_tokens_for_text(context, model.as_str(), request_text.as_str()).await);
        output_tokens =
            Some(estimate_tokens_for_text(context, model.as_str(), response_text.as_str()).await);
    }

    let usage_event = UsageWrite {
        at_unix_ms: now_unix_ms_i64(),
        provider_id: context.provider_id,
        credential_id: context.credential_id,
        user_id: Some(context.auth.user_id),
        user_key_id: Some(context.auth.user_key_id),
        operation: format!("{:?}", context.request.operation()),
        protocol: format!("{:?}", context.request.protocol()),
        model: request_model,
        input_tokens: input_tokens.map(|value| value.max(0)),
        output_tokens: output_tokens.map(|value| value.max(0)),
        cache_read_input_tokens,
        cache_creation_input_tokens,
    };
    if let Err(err) = context
        .state
        .enqueue_storage_write(StorageWriteEvent::UpsertUsage(usage_event))
        .await
    {
        eprintln!("provider: stream usage event enqueue failed: {err}");
    }
}

fn serialize_claude_model(model: &ClaudeModel) -> Option<String> {
    serde_json::to_value(model)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

fn serialize_openai_embedding_model(model: &OpenAiEmbeddingModel) -> Option<String> {
    serde_json::to_value(model)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

fn extract_model_from_request(request: &TransformRequest) -> Option<String> {
    match request {
        TransformRequest::ModelListOpenAi(_)
        | TransformRequest::ModelListClaude(_)
        | TransformRequest::ModelListGemini(_) => None,

        TransformRequest::ModelGetOpenAi(value) => Some(value.path.model.clone()),
        TransformRequest::ModelGetClaude(value) => Some(value.path.model_id.clone()),
        TransformRequest::ModelGetGemini(value) => Some(value.path.name.clone()),

        TransformRequest::CountTokenOpenAi(value) => value.body.model.clone(),
        TransformRequest::CountTokenClaude(value) => serialize_claude_model(&value.body.model),
        TransformRequest::CountTokenGemini(value) => {
            if let Some(generate_request) = value.body.generate_content_request.as_ref() {
                Some(generate_request.model.clone())
            } else {
                Some(value.path.model.clone())
            }
        }

        TransformRequest::GenerateContentOpenAiResponse(value)
        | TransformRequest::StreamGenerateContentOpenAiResponse(value) => value.body.model.clone(),

        TransformRequest::GenerateContentOpenAiChatCompletions(value)
        | TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
            Some(value.body.model.clone())
        }

        TransformRequest::GenerateContentClaude(value)
        | TransformRequest::StreamGenerateContentClaude(value) => {
            serialize_claude_model(&value.body.model)
        }

        TransformRequest::GenerateContentGemini(value) => Some(value.path.model.clone()),
        TransformRequest::StreamGenerateContentGeminiSse(value)
        | TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
            Some(value.path.model.clone())
        }

        TransformRequest::EmbeddingOpenAi(value) => {
            serialize_openai_embedding_model(&value.body.model)
        }
        TransformRequest::EmbeddingGemini(value) => Some(value.path.model.clone()),

        TransformRequest::CompactOpenAi(value) => Some(value.body.model.clone()),
    }
}

fn normalize_usage_model(model: Option<String>) -> Option<String> {
    model.and_then(|value| {
        let trimmed = value.trim().trim_start_matches('/');
        if trimmed.is_empty() {
            return None;
        }
        let normalized = if let Some(stripped) = trimmed.strip_prefix("models/") {
            stripped.trim()
        } else {
            trimmed
        };
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.to_string())
        }
    })
}

struct UpstreamAndUsageEventInput<'a> {
    auth: RequestAuthContext,
    request: &'a TransformRequest,
    provider_id: Option<i64>,
    credential_id: Option<i64>,
    request_meta: Option<&'a UpstreamRequestMeta>,
    error_status: Option<u16>,
    response_status: Option<u16>,
    response_headers: &'a [(String, String)],
    response_body: Option<Vec<u8>>,
    local_response: Option<&'a gproxy_middleware::TransformResponse>,
}

async fn enqueue_upstream_and_usage_event(state: &AppState, input: UpstreamAndUsageEventInput<'_>) {
    let UpstreamAndUsageEventInput {
        auth,
        request,
        provider_id,
        credential_id,
        request_meta,
        error_status,
        response_status,
        response_headers,
        response_body,
        local_response,
    } = input;
    let operation = format!("{:?}", request.operation());
    let protocol = format!("{:?}", request.protocol());
    let request_model = normalize_usage_model(extract_model_from_request(request));
    let now_unix_ms = now_unix_ms_i64();
    let extracted_usage = local_response.and_then(extract_usage_from_local_response);
    let mask_sensitive_info = state.config.load().global.mask_sensitive_info;
    let persisted_request_body = if mask_sensitive_info {
        None
    } else {
        request_meta.and_then(|meta| meta.body.clone())
    };
    let persisted_response_body = if mask_sensitive_info {
        None
    } else {
        response_body.or_else(|| local_response.and_then(|value| serde_json::to_vec(value).ok()))
    };
    if let Some(meta) = request_meta {
        let upstream_event = UpstreamRequestWrite {
            at_unix_ms: now_unix_ms,
            internal: false,
            provider_id,
            credential_id,
            request_method: meta.method.clone(),
            request_headers_json: headers_pairs_to_json(meta.headers.as_slice()),
            request_url: Some(meta.url.clone()),
            request_body: persisted_request_body,
            response_status: response_status.or(error_status).map(i32::from),
            response_headers_json: headers_pairs_to_json(response_headers),
            response_body: persisted_response_body,
        };
        if let Err(err) = state
            .enqueue_storage_write(StorageWriteEvent::UpsertUpstreamRequest(upstream_event))
            .await
        {
            eprintln!("provider: upstream event enqueue failed: {err}");
        }
    }

    if !should_record_usage(request.operation())
        || response_status.map(|value| value >= 400).unwrap_or(true)
    {
        return;
    }
    if request.operation() == OperationFamily::StreamGenerateContent {
        return;
    }

    let mut input_tokens = extracted_usage.and_then(|value| value.input_tokens);
    let mut output_tokens = extracted_usage.and_then(|value| value.output_tokens);
    let cache_read_input_tokens = extracted_usage.and_then(|value| value.cache_read_input_tokens);
    let cache_creation_input_tokens =
        extracted_usage.and_then(|value| value.cache_creation_input_tokens);

    if request.operation() == OperationFamily::Embedding && input_tokens.is_none() {
        input_tokens = estimate_embedding_input_tokens_from_request(state, request).await;
        output_tokens = output_tokens.or(Some(0));
    }
    if request.operation() == OperationFamily::CountToken && input_tokens.is_some() {
        output_tokens = Some(0);
    }

    let usage_event = UsageWrite {
        at_unix_ms: now_unix_ms,
        provider_id,
        credential_id,
        user_id: Some(auth.user_id),
        user_key_id: Some(auth.user_key_id),
        operation,
        protocol,
        model: request_model,
        input_tokens,
        output_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
    };
    if let Err(err) = state
        .enqueue_storage_write(StorageWriteEvent::UpsertUsage(usage_event))
        .await
    {
        eprintln!("provider: usage event enqueue failed: {err}");
    }
}

async fn enqueue_upstream_request_event_from_meta(
    state: &AppState,
    provider_id: Option<i64>,
    credential_id: Option<i64>,
    request_meta: Option<&UpstreamRequestMeta>,
    response_status: Option<u16>,
    response_headers: &[(String, String)],
    response_body: Option<Vec<u8>>,
) {
    let Some(meta) = request_meta else {
        return;
    };
    let mask_sensitive_info = state.config.load().global.mask_sensitive_info;
    let upstream_event = UpstreamRequestWrite {
        at_unix_ms: now_unix_ms_i64(),
        internal: false,
        provider_id,
        credential_id,
        request_method: meta.method.clone(),
        request_headers_json: headers_pairs_to_json(meta.headers.as_slice()),
        request_url: Some(meta.url.clone()),
        request_body: if mask_sensitive_info {
            None
        } else {
            meta.body.clone()
        },
        response_status: response_status.map(i32::from),
        response_headers_json: headers_pairs_to_json(response_headers),
        response_body: if mask_sensitive_info {
            None
        } else {
            response_body
        },
    };
    if let Err(err) = state
        .enqueue_storage_write(StorageWriteEvent::UpsertUpstreamRequest(upstream_event))
        .await
    {
        eprintln!("provider: upstream event enqueue failed: {err}");
    }
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

fn websocket_upgrade_required_response(message: &str) -> Response {
    let body = serde_json::to_vec(&json!({
        "error": {
            "message": message,
            "type": "upgrade_required",
            "code": "websocket_upgrade_required"
        }
    }))
    .unwrap_or_default();

    Response::builder()
        .status(StatusCode::UPGRADE_REQUIRED)
        .header("content-type", "application/json")
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::from("failed to build websocket upgrade response")))
}

fn should_rewrite_gemini_stream_to_ndjson(request: &TransformRequest) -> bool {
    matches!(
        request,
        TransformRequest::StreamGenerateContentGeminiNdjson(_)
    )
}

fn is_sse_content_type(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("content-type")
            && value.to_ascii_lowercase().contains("text/event-stream")
    })
}

fn is_streaming_content_type(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("content-type") && {
            let content_type = value.to_ascii_lowercase();
            content_type.contains("text/event-stream")
                || content_type.contains("application/x-ndjson")
        }
    })
}

fn rewrite_content_type_to_ndjson(headers: &mut Vec<(String, String)>) {
    let mut replaced = false;
    for (name, value) in headers.iter_mut() {
        if name.eq_ignore_ascii_case("content-type") {
            *value = "application/x-ndjson".to_string();
            replaced = true;
        }
    }
    if !replaced {
        headers.push((
            "content-type".to_string(),
            "application/x-ndjson".to_string(),
        ));
    }
}

fn remove_header_ignore_case(headers: &mut Vec<(String, String)>, header_name: &str) {
    headers.retain(|(name, _)| !name.eq_ignore_ascii_case(header_name));
}

fn transformed_payload_content_type(
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> &'static str {
    if operation != OperationFamily::StreamGenerateContent {
        return "application/json";
    }
    match protocol {
        ProtocolKind::GeminiNDJson => "application/x-ndjson",
        _ => "text/event-stream",
    }
}

fn rewrite_content_type(headers: &mut Vec<(String, String)>, content_type: &str) {
    let mut replaced = false;
    for (name, value) in headers.iter_mut() {
        if name.eq_ignore_ascii_case("content-type") {
            *value = content_type.to_string();
            replaced = true;
        }
    }
    if !replaced {
        headers.push(("content-type".to_string(), content_type.to_string()));
    }
    remove_header_ignore_case(headers, "content-length");
}

fn wrap_stream_with_upstream_record(
    input: std::pin::Pin<
        Box<dyn Stream<Item = Result<Bytes, MiddlewareTransformError>> + Send + 'static>,
    >,
    context: UpstreamStreamRecordContext,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, MiddlewareTransformError>> + Send + 'static>>
{
    let (tx, mut rx) = mpsc::channel::<Result<Bytes, MiddlewareTransformError>>(16);
    tokio::spawn(async move {
        let usage_extracted = attach_usage_extractor(TransformResponsePayload::new(
            context.request.operation(),
            context.request.protocol(),
            input,
        ));
        let mut context = context;
        context.stream_usage = Some(usage_extracted.usage.clone());
        let mut input = usage_extracted.response.body;
        let recorder = UpstreamStreamRecordGuard::new(context);
        let mut downstream_closed = false;
        while let Some(item) = input.next().await {
            match item {
                Ok(chunk) => {
                    recorder.push_chunk(chunk.as_ref());
                    if !downstream_closed
                        && tx
                            .send(Ok::<Bytes, MiddlewareTransformError>(chunk))
                            .await
                            .is_err()
                    {
                        downstream_closed = true;
                    }
                }
                Err(err) => {
                    if !downstream_closed {
                        let _ = tx.send(Err::<Bytes, MiddlewareTransformError>(err)).await;
                    }
                    break;
                }
            }
        }
        recorder.flush_now().await;
    });
    Box::pin(async_stream::stream! {
        while let Some(item) = rx.recv().await {
            yield item;
        }
    })
}

fn wrap_io_stream_with_upstream_record(
    input: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static>,
    >,
    context: UpstreamStreamRecordContext,
) -> std::pin::Pin<
    Box<dyn futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static>,
> {
    let (tx, mut rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(16);
    tokio::spawn(async move {
        let transformed_stream = input.map(|item| {
            item.map_err(|err| MiddlewareTransformError::ProviderPrefix {
                message: err.to_string(),
            })
        });
        let usage_extracted = attach_usage_extractor(TransformResponsePayload::new(
            context.request.operation(),
            context.request.protocol(),
            Box::pin(transformed_stream),
        ));
        let mut context = context;
        context.stream_usage = Some(usage_extracted.usage.clone());
        let mut input = usage_extracted.response.body;
        let recorder = UpstreamStreamRecordGuard::new(context);
        let mut downstream_closed = false;
        while let Some(item) = input.next().await {
            match item {
                Ok(chunk) => {
                    recorder.push_chunk(chunk.as_ref());
                    if !downstream_closed
                        && tx.send(Ok::<Bytes, std::io::Error>(chunk)).await.is_err()
                    {
                        downstream_closed = true;
                    }
                }
                Err(err) => {
                    if !downstream_closed {
                        let _ = tx
                            .send(Err::<Bytes, std::io::Error>(std::io::Error::other(
                                err.to_string(),
                            )))
                            .await;
                    }
                    break;
                }
            }
        }
        recorder.flush_now().await;
    });
    Box::pin(async_stream::stream! {
        while let Some(item) = rx.recv().await {
            yield item;
        }
    })
}

fn unwrap_http_wrapper_body_bytes(body: &[u8]) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    let wrapper = value.as_object()?;
    if !wrapper.contains_key("stats_code") || !wrapper.contains_key("body") {
        return None;
    }
    match wrapper.get("body")? {
        serde_json::Value::String(text) => Some(text.as_bytes().to_vec()),
        body => serde_json::to_vec(body).ok(),
    }
}

async fn transformed_payload_to_axum_response(
    status: StatusCode,
    mut headers: Vec<(String, String)>,
    payload: TransformResponsePayload,
    stream_record_context: Option<UpstreamStreamRecordContext>,
) -> Result<Response, UpstreamError> {
    let content_type = transformed_payload_content_type(payload.operation, payload.protocol);
    rewrite_content_type(&mut headers, content_type);
    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    if payload.operation != OperationFamily::StreamGenerateContent {
        let mut body = payload.body;
        let mut collected = Vec::new();
        while let Some(item) = body.next().await {
            let chunk = item.map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            collected.extend_from_slice(chunk.as_ref());
        }
        let client_body = unwrap_http_wrapper_body_bytes(collected.as_slice()).unwrap_or(collected);
        return builder
            .body(Body::from(client_body))
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()));
    }
    let body = if let Some(context) = stream_record_context {
        wrap_stream_with_upstream_record(payload.body, context)
    } else {
        payload.body
    };
    let body_stream = body.map(|item| item.map_err(|err| std::io::Error::other(err.to_string())));
    builder
        .body(Body::from_stream(body_stream))
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

fn response_from_status_headers_and_bytes(
    status: StatusCode,
    headers: &[(String, String)],
    body: Vec<u8>,
) -> Result<Response, UpstreamError> {
    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder
        .body(Body::from(body))
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

fn ensure_stream_usage_option_on_native_chat(request: &mut TransformRequest) {
    if let TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) = request {
        let options = value
            .body
            .stream_options
            .get_or_insert_with(Default::default);
        options.include_usage = Some(true);
    }
}

fn encode_http_response_for_transform(
    status: StatusCode,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<Vec<u8>, UpstreamError> {
    let mut header_map = serde_json::Map::new();
    for (name, value) in headers {
        header_map.insert(name.clone(), serde_json::Value::String(value.clone()));
    }
    let body_json = serde_json::from_slice::<serde_json::Value>(body)
        .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(body).to_string()));
    serde_json::to_vec(&json!({
        "stats_code": status.as_u16(),
        "headers": header_map,
        "body": body_json,
    }))
    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

fn upstream_response_to_axum_stream(
    response: wreq::Response,
    rewrite_gemini_stream_to_ndjson: bool,
    stream_record_context: Option<UpstreamStreamRecordContext>,
) -> Result<Response, UpstreamError> {
    let stream_channel = stream_record_context
        .as_ref()
        .map(|value| value.channel.clone());
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect::<Vec<_>>();

    let is_sse = is_sse_content_type(headers.as_slice());
    let rewrite_stream = rewrite_gemini_stream_to_ndjson && is_sse;
    let unwrap_sse = !rewrite_stream
        && is_sse
        && stream_channel
            .as_ref()
            .map(is_wrapped_stream_channel)
            .unwrap_or(false);
    if rewrite_stream {
        rewrite_content_type_to_ndjson(&mut headers);
        remove_header_ignore_case(&mut headers, "content-length");
    } else if unwrap_sse {
        remove_header_ignore_case(&mut headers, "content-length");
    }

    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    if rewrite_stream || unwrap_sse {
        let mut upstream_stream = response.bytes_stream();
        let mut rewriter = SseToNdjsonRewriter::default();
        let base_stream = async_stream::stream! {
            while let Some(item) = upstream_stream.next().await {
                let chunk = match item {
                    Ok(chunk) => chunk,
                    Err(err) => {
                        yield Err::<Bytes, std::io::Error>(std::io::Error::other(err.to_string()));
                        return;
                    }
                };
                let out = rewriter.push_chunk(chunk.as_ref());
                if !out.is_empty() {
                    let normalized = stream_channel
                        .as_ref()
                        .and_then(|channel| {
                            normalize_upstream_stream_ndjson_chunk_for_channel(channel, out.as_slice())
                        })
                        .unwrap_or(out);
                    let output = if rewrite_stream {
                        normalized
                    } else {
                        ndjson_chunk_to_sse_chunk(normalized.as_slice())
                    };
                    if !output.is_empty() {
                        yield Ok::<Bytes, std::io::Error>(Bytes::from(output));
                    }
                }
            }
            let tail = rewriter.finish();
            if !tail.is_empty() {
                let normalized_tail = stream_channel
                    .as_ref()
                    .and_then(|channel| {
                        normalize_upstream_stream_ndjson_chunk_for_channel(
                            channel,
                            tail.as_slice(),
                        )
                    })
                    .unwrap_or(tail);
                let output_tail = if rewrite_stream {
                    normalized_tail
                } else {
                    ndjson_chunk_to_sse_chunk(normalized_tail.as_slice())
                };
                if !output_tail.is_empty() {
                    yield Ok::<Bytes, std::io::Error>(Bytes::from(output_tail));
                }
            }
        };
        let body_stream = if let Some(context) = stream_record_context {
            wrap_io_stream_with_upstream_record(Box::pin(base_stream), context)
        } else {
            Box::pin(base_stream)
        };
        return builder
            .body(Body::from_stream(body_stream))
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()));
    }

    let base_body_stream = response.bytes_stream().map(|item| {
        item.map_err(|err| MiddlewareTransformError::ProviderPrefix {
            message: err.to_string(),
        })
    });
    let base_body_stream = if let Some(context) = stream_record_context {
        wrap_stream_with_upstream_record(Box::pin(base_body_stream), context)
    } else {
        Box::pin(base_body_stream)
    }
    .map(|item| item.map_err(|err| std::io::Error::other(err.to_string())));
    builder
        .body(Body::from_stream(base_body_stream))
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
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

fn parse_json_body<T: DeserializeOwned>(body: &Bytes, context: &str) -> Result<T, HttpError> {
    serde_json::from_slice(body).map_err(|err| bad_request(format!("{context}: {err}")))
}

fn serialize_json_scalar<T: serde::Serialize>(
    value: &T,
    context: &str,
) -> Result<String, HttpError> {
    let value = serde_json::to_value(value)
        .map_err(|err| bad_request(format!("invalid {context}: {err}")))?;
    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| bad_request(format!("{context} must be a string")))
}

fn deserialize_json_scalar<T: DeserializeOwned>(
    value: &str,
    context: &str,
) -> Result<T, HttpError> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .map_err(|err| bad_request(format!("invalid {context}: {err}")))
}

fn split_provider_prefixed_plain_model(raw: &str) -> Result<(String, String), HttpError> {
    split_provider_prefixed_model(raw, false)
}

fn split_provider_prefixed_model_path(raw: &str) -> Result<(String, String), HttpError> {
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

fn split_provider_prefixed_gemini_target(target: &str) -> Result<(String, String), HttpError> {
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

fn anthropic_headers_from_request(
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

fn normalize_gemini_model_path(value: &str) -> Result<String, HttpError> {
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

fn next_incremental_id(values: impl Iterator<Item = i64>) -> i64 {
    values.max().unwrap_or(-1) + 1
}

async fn resolve_provider_id(state: &AppState, channel: &ChannelId) -> Result<i64, HttpError> {
    let storage = state.load_storage();
    let rows = storage
        .list_providers(&ProviderQuery {
            channel: Scope::Eq(channel.as_str().to_string()),
            name: Scope::All,
            enabled: Scope::All,
            limit: Some(1),
        })
        .await
        .map_err(|err| internal_error(err.to_string()))?;
    if let Some(row) = rows.into_iter().next() {
        return Ok(row.id);
    }

    let all = storage
        .list_providers(&ProviderQuery {
            channel: Scope::All,
            name: Scope::All,
            enabled: Scope::All,
            limit: None,
        })
        .await
        .map_err(|err| internal_error(err.to_string()))?;
    Ok(next_incremental_id(all.into_iter().map(|row| row.id)))
}

async fn resolve_credential_id(
    state: &AppState,
    provider_id: i64,
    credential: &CredentialRef,
) -> Result<i64, HttpError> {
    let storage = state.load_storage();
    let expected_name = credential
        .label
        .clone()
        .unwrap_or_else(|| credential.id.to_string());
    let rows = storage
        .list_credentials(&CredentialQuery {
            provider_id: Scope::Eq(provider_id),
            kind: Scope::All,
            enabled: Scope::All,
            limit: Some(256),
        })
        .await
        .map_err(|err| internal_error(err.to_string()))?;

    if let Some(row) = rows
        .into_iter()
        .find(|row| row.name.as_deref() == Some(expected_name.as_str()))
    {
        return Ok(row.id);
    }

    let all_credentials = storage
        .list_credentials(&CredentialQuery {
            provider_id: Scope::All,
            kind: Scope::All,
            enabled: Scope::All,
            limit: None,
        })
        .await
        .map_err(|err| internal_error(err.to_string()))?;

    Ok(next_incremental_id(
        all_credentials.into_iter().map(|row| row.id),
    ))
}

fn channel_credential_kind(credential: &ChannelCredential) -> String {
    match credential {
        ChannelCredential::Builtin(BuiltinChannelCredential::OpenAi(_)) => "builtin/openai",
        ChannelCredential::Builtin(BuiltinChannelCredential::Claude(_)) => "builtin/claude",
        ChannelCredential::Builtin(BuiltinChannelCredential::AiStudio(_)) => "builtin/aistudio",
        ChannelCredential::Builtin(BuiltinChannelCredential::VertexExpress(_)) => {
            "builtin/vertexexpress"
        }
        ChannelCredential::Builtin(BuiltinChannelCredential::Vertex(_)) => "builtin/vertex",
        ChannelCredential::Builtin(BuiltinChannelCredential::GeminiCli(_)) => "builtin/geminicli",
        ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(_)) => "builtin/claudecode",
        ChannelCredential::Builtin(BuiltinChannelCredential::Codex(_)) => "builtin/codex",
        ChannelCredential::Builtin(BuiltinChannelCredential::Antigravity(_)) => {
            "builtin/antigravity"
        }
        ChannelCredential::Builtin(BuiltinChannelCredential::Nvidia(_)) => "builtin/nvidia",
        ChannelCredential::Builtin(BuiltinChannelCredential::Deepseek(_)) => "builtin/deepseek",
        ChannelCredential::Custom(_) => "custom/apikey",
    }
    .to_string()
}

async fn persist_provider_and_credential(
    state: &AppState,
    channel: &ChannelId,
    provider: &ProviderDefinition,
    credential: &CredentialRef,
) -> Result<(), HttpError> {
    let provider_id = resolve_provider_id(state, channel).await?;
    let provider_settings_json =
        gproxy_provider::provider_settings_to_json_string(&provider.settings)
            .map_err(|err| internal_error(err.to_string()))?;
    let provider_dispatch_json =
        serde_json::to_string(&provider.dispatch).map_err(|err| internal_error(err.to_string()))?;

    let provider_write = ProviderWrite {
        id: provider_id,
        name: channel.as_str().to_string(),
        channel: channel.as_str().to_string(),
        settings_json: provider_settings_json,
        dispatch_json: provider_dispatch_json,
        enabled: true,
    };
    let credential_secret_json = serde_json::to_string(&credential.credential)
        .map_err(|err| internal_error(err.to_string()))?;
    let credential_write = CredentialWrite {
        id: credential.id,
        provider_id,
        name: credential
            .label
            .clone()
            .or_else(|| Some(credential.id.to_string())),
        kind: channel_credential_kind(&credential.credential),
        settings_json: None,
        secret_json: credential_secret_json,
        enabled: true,
    };
    let mut batch = StorageWriteBatch::default();
    batch.apply(StorageWriteEvent::UpsertProvider(provider_write));
    batch.apply(StorageWriteEvent::UpsertCredential(credential_write));
    state
        .load_storage()
        .write_batch(batch)
        .await
        .map_err(|err| internal_error(err.to_string()))
}

async fn apply_credential_update_and_persist(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    update: UpstreamCredentialUpdate,
) {
    if !state.apply_upstream_credential_update_in_memory(&channel, &update) {
        eprintln!(
            "provider: skip credential update, in-memory apply failed channel={} credential_id={}",
            channel.as_str(),
            update.credential_id()
        );
        return;
    }
    let Some(credential) =
        state.get_provider_credential_in_memory(&channel, update.credential_id())
    else {
        eprintln!(
            "provider: skip credential update, updated credential missing in-memory channel={} credential_id={}",
            channel.as_str(),
            update.credential_id()
        );
        return;
    };

    if let Err(err) =
        persist_provider_and_credential(&state, &channel, &provider, &credential).await
    {
        eprintln!(
            "provider: persist credential update failed channel={} credential_id={} error={:?}",
            channel.as_str(),
            credential.id,
            err
        );
    }
}

fn build_openai_local_count_response(
    input_tokens: u64,
) -> openai_count_tokens_response::OpenAiCountTokensResponse {
    openai_count_tokens_response::OpenAiCountTokensResponse::Success {
        stats_code: StatusCode::OK,
        headers: gproxy_protocol::openai::types::OpenAiResponseHeaders::default(),
        body: openai_count_tokens_response::ResponseBody {
            input_tokens,
            object: openai_count_tokens_response::OpenAiCountTokensObject::ResponseInputTokens,
        },
    }
}

async fn execute_local_count_token_request(
    state: &AppState,
    request: &TransformRequest,
) -> Result<UpstreamResponse, UpstreamError> {
    let openai_request = match request {
        TransformRequest::CountTokenOpenAi(value) => value.clone(),
        TransformRequest::CountTokenClaude(value) => {
            openai_count_tokens_request::OpenAiCountTokensRequest::try_from(value.clone())
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?
        }
        TransformRequest::CountTokenGemini(value) => {
            openai_count_tokens_request::OpenAiCountTokensRequest::try_from(value.clone())
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?
        }
        _ => return Err(UpstreamError::UnsupportedRequest),
    };

    let mut normalized = serde_json::to_value(&openai_request.body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    if let Some(object) = normalized.as_object_mut() {
        object.remove("model");
    }
    let text = serde_json::to_string(&normalized)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let model = openai_request
        .body
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("deepseek_fallback");

    let token_count = state
        .count_tokens_with_local_tokenizer(model, text.as_str())
        .await
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?
        .count as u64;

    let response = match request {
        TransformRequest::CountTokenOpenAi(_) => {
            gproxy_middleware::TransformResponse::CountTokenOpenAi(
                build_openai_local_count_response(token_count),
            )
        }
        TransformRequest::CountTokenClaude(_) => {
            gproxy_middleware::TransformResponse::CountTokenClaude(
                claude_count_tokens_response::ClaudeCountTokensResponse::try_from(
                    build_openai_local_count_response(token_count),
                )
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
            )
        }
        TransformRequest::CountTokenGemini(_) => {
            gproxy_middleware::TransformResponse::CountTokenGemini(
                gemini_count_tokens_response::GeminiCountTokensResponse::try_from(
                    build_openai_local_count_response(token_count),
                )
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
            )
        }
        _ => return Err(UpstreamError::UnsupportedRequest),
    };

    Ok(UpstreamResponse::from_local(response))
}

async fn execute_local_request(
    state: &AppState,
    channel: &ChannelId,
    request: &TransformRequest,
) -> Result<UpstreamResponse, UpstreamError> {
    if let ChannelId::Builtin(BuiltinChannel::VertexExpress) = channel
        && let Some(local) = try_local_vertexexpress_model_response(request)?
    {
        return Ok(UpstreamResponse::from_local(local));
    }

    execute_local_count_token_request(state, request).await
}

async fn execute_transform_request(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    request: TransformRequest,
) -> Result<Response, UpstreamError> {
    let downstream_request = request;
    let mut upstream_request = downstream_request.clone();
    let mut dispatch_route = None;
    let mut dispatch_local = false;
    let provider_id = resolve_provider_id(state.as_ref(), &channel).await.ok();
    let src_route = RouteKey::new(
        downstream_request.operation(),
        downstream_request.protocol(),
    );
    let Some(implementation) = provider.dispatch.resolve(src_route).cloned() else {
        enqueue_upstream_and_usage_event(
            state.as_ref(),
            UpstreamAndUsageEventInput {
                auth,
                request: &downstream_request,
                provider_id,
                credential_id: None,
                request_meta: None,
                error_status: None,
                response_status: None,
                response_headers: &[],
                response_body: None,
                local_response: None,
            },
        )
        .await;
        return Err(UpstreamError::UnsupportedRequest);
    };

    match implementation {
        RouteImplementation::Passthrough => {}
        RouteImplementation::TransformTo { destination } => {
            let route = gproxy_middleware::TransformRoute {
                src_operation: src_route.operation,
                src_protocol: src_route.protocol,
                dst_operation: destination.operation,
                dst_protocol: destination.protocol,
            };
            if !route.is_passthrough() {
                match gproxy_middleware::transform_request(downstream_request.clone(), route) {
                    Ok(transformed) => {
                        upstream_request = transformed;
                    }
                    Err(err) => {
                        let upstream_error = UpstreamError::SerializeRequest(err.to_string());
                        enqueue_upstream_and_usage_event(
                            state.as_ref(),
                            UpstreamAndUsageEventInput {
                                auth,
                                request: &downstream_request,
                                provider_id,
                                credential_id: None,
                                request_meta: None,
                                error_status: None,
                                response_status: None,
                                response_headers: &[],
                                response_body: None,
                                local_response: None,
                            },
                        )
                        .await;
                        return Err(upstream_error);
                    }
                }
            }
            dispatch_route = Some(route);
        }
        RouteImplementation::Local => {
            dispatch_local = true;
        }
        RouteImplementation::Unsupported => {
            enqueue_upstream_and_usage_event(
                state.as_ref(),
                UpstreamAndUsageEventInput {
                    auth,
                    request: &downstream_request,
                    provider_id,
                    credential_id: None,
                    request_meta: None,
                    error_status: None,
                    response_status: None,
                    response_headers: &[],
                    response_body: None,
                    local_response: None,
                },
            )
            .await;
            return Err(UpstreamError::UnsupportedRequest);
        }
    }

    let now = now_unix_ms();
    ensure_stream_usage_option_on_native_chat(&mut upstream_request);
    let upstream_result = if dispatch_local {
        execute_local_request(state.as_ref(), &channel, &downstream_request).await
    } else {
        let http = if matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode)) {
            state.load_spoof_http()
        } else {
            state.load_http()
        };
        let tokenizers = state.tokenizers();
        let global = state.config.load().global.clone();

        provider
            .execute_with_retry(
                http.as_ref(),
                &state.credential_states,
                &upstream_request,
                now,
                TokenizerResolutionContext {
                    tokenizer_store: tokenizers.as_ref(),
                    hf_token: global.hf_token.as_deref(),
                    hf_url: global.hf_url.as_deref(),
                },
            )
            .await
    };
    if !dispatch_local {
        enqueue_credential_status_updates_for_request(state.as_ref(), &channel, &provider, now)
            .await;
    }
    let upstream = match upstream_result {
        Ok(value) => value,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            let err_credential_id = upstream_error_credential_id(&err);
            let err_status = upstream_error_status(&err);
            enqueue_upstream_and_usage_event(
                state.as_ref(),
                UpstreamAndUsageEventInput {
                    auth,
                    request: &downstream_request,
                    provider_id,
                    credential_id: err_credential_id,
                    request_meta: err_request_meta.as_ref(),
                    error_status: err_status,
                    response_status: None,
                    response_headers: &[],
                    response_body: None,
                    local_response: None,
                },
            )
            .await;
            return Err(err);
        }
    };
    let upstream_credential_id = upstream.credential_id;
    let upstream_request_meta = upstream.request_meta.clone();

    if let Some(update) = upstream.credential_update.clone() {
        apply_credential_update_and_persist(
            state.clone(),
            channel.clone(),
            provider.clone(),
            update,
        )
        .await;
    }

    if let Some(mut local) = upstream.local_response {
        let usage_source_response = local.clone();
        if let Some(route) = dispatch_route.filter(|item| !item.is_passthrough()) {
            local = gproxy_middleware::transform_response(local, route)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        }
        enqueue_upstream_and_usage_event(
            state.as_ref(),
            UpstreamAndUsageEventInput {
                auth,
                request: &downstream_request,
                provider_id,
                credential_id: upstream_credential_id,
                request_meta: upstream_request_meta.as_ref(),
                error_status: None,
                response_status: Some(200),
                response_headers: &[],
                response_body: None,
                local_response: Some(&usage_source_response),
            },
        )
        .await;
        let body = serde_json::to_vec(&local)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let response = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        return Ok(response);
    }

    if let Some(response) = upstream.response {
        let response_status = response.status().as_u16();
        let response_headers = response_headers_to_pairs(&response);
        if let Some(route) = dispatch_route.filter(|item| !item.is_passthrough()) {
            if !response.status().is_success() {
                let stream_record_context = UpstreamStreamRecordContext {
                    state: state.clone(),
                    channel: channel.clone(),
                    provider: provider.clone(),
                    auth,
                    request: downstream_request.clone(),
                    provider_id,
                    credential_id: upstream_credential_id,
                    request_meta: upstream_request_meta.clone(),
                    response_status: Some(response_status),
                    response_headers: response_headers.clone(),
                    stream_usage: None,
                };
                return upstream_response_to_axum_stream(
                    response,
                    false,
                    Some(stream_record_context),
                );
            }
            let status =
                StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let headers = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.as_str().to_string(), value.to_string()))
                })
                .collect::<Vec<_>>();
            let transformed_payload = if route.dst_operation
                == OperationFamily::StreamGenerateContent
            {
                let body_stream = response.bytes_stream().map(|item| {
                    item.map_err(|err| MiddlewareTransformError::ProviderPrefix {
                        message: err.to_string(),
                    })
                });
                let body_stream: std::pin::Pin<
                    Box<
                        dyn Stream<Item = Result<Bytes, MiddlewareTransformError>> + Send + 'static,
                    >,
                > = if is_wrapped_stream_channel(&channel)
                    && matches!(
                        route.dst_protocol,
                        ProtocolKind::Gemini | ProtocolKind::GeminiNDJson
                    ) {
                    let mut upstream_stream = Box::pin(body_stream);
                    let wrapped_channel = channel.clone();
                    let dst_protocol = route.dst_protocol;
                    Box::pin(async_stream::stream! {
                        let mut rewriter = SseToNdjsonRewriter::default();
                        while let Some(item) = upstream_stream.next().await {
                            let chunk = match item {
                                Ok(chunk) => chunk,
                                Err(err) => {
                                    yield Err::<Bytes, MiddlewareTransformError>(err);
                                    return;
                                }
                            };
                            let out = rewriter.push_chunk(chunk.as_ref());
                            if !out.is_empty() {
                                let normalized = normalize_upstream_stream_ndjson_chunk_for_channel(
                                    &wrapped_channel,
                                    out.as_slice(),
                                )
                                .unwrap_or(out);
                                let emitted = if dst_protocol == ProtocolKind::Gemini {
                                    ndjson_chunk_to_sse_chunk(normalized.as_slice())
                                } else {
                                    normalized
                                };
                                if !emitted.is_empty() {
                                    yield Ok::<Bytes, MiddlewareTransformError>(Bytes::from(emitted));
                                }
                            }
                        }
                        let tail = rewriter.finish();
                        if !tail.is_empty() {
                            let normalized_tail = normalize_upstream_stream_ndjson_chunk_for_channel(
                                &wrapped_channel,
                                tail.as_slice(),
                            )
                            .unwrap_or(tail);
                            let emitted_tail = if dst_protocol == ProtocolKind::Gemini {
                                ndjson_chunk_to_sse_chunk(normalized_tail.as_slice())
                            } else {
                                normalized_tail
                            };
                            if !emitted_tail.is_empty() {
                                yield Ok::<Bytes, MiddlewareTransformError>(Bytes::from(emitted_tail));
                            }
                        }
                    })
                } else {
                    Box::pin(body_stream)
                };
                gproxy_middleware::transform_response_payload(
                    TransformResponsePayload::new(
                        route.dst_operation,
                        route.dst_protocol,
                        body_stream,
                    ),
                    route,
                )
                .await
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?
            } else {
                let body_bytes = response
                    .bytes()
                    .await
                    .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
                let raw_body = body_bytes.to_vec();
                let normalized_body =
                    normalize_upstream_response_body_for_channel(&channel, body_bytes.as_ref())
                        .unwrap_or_else(|| raw_body.clone());
                let encoded = encode_http_response_for_transform(
                    status,
                    headers.as_slice(),
                    normalized_body.as_ref(),
                )?;
                let usage_source_response = decode_response_for_usage(
                    route.dst_operation,
                    route.dst_protocol,
                    encoded.as_ref(),
                );
                enqueue_upstream_and_usage_event(
                    state.as_ref(),
                    UpstreamAndUsageEventInput {
                        auth,
                        request: &downstream_request,
                        provider_id,
                        credential_id: upstream_credential_id,
                        request_meta: upstream_request_meta.as_ref(),
                        error_status: None,
                        response_status: Some(response_status),
                        response_headers: response_headers.as_slice(),
                        response_body: Some(raw_body),
                        local_response: usage_source_response.as_ref(),
                    },
                )
                .await;
                let body_stream =
                    futures_util::stream::once(async move { Ok(Bytes::from(encoded)) });
                gproxy_middleware::transform_response_payload(
                    TransformResponsePayload::new(
                        route.dst_operation,
                        route.dst_protocol,
                        Box::pin(body_stream),
                    ),
                    route,
                )
                .await
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?
            };
            let stream_record_context = (route.dst_operation
                == OperationFamily::StreamGenerateContent)
                .then(|| UpstreamStreamRecordContext {
                    state: state.clone(),
                    channel: channel.clone(),
                    provider: provider.clone(),
                    auth,
                    request: downstream_request.clone(),
                    provider_id,
                    credential_id: upstream_credential_id,
                    request_meta: upstream_request_meta.clone(),
                    response_status: Some(response_status),
                    response_headers: response_headers.clone(),
                    stream_usage: None,
                });
            return transformed_payload_to_axum_response(
                status,
                headers,
                transformed_payload,
                stream_record_context,
            )
            .await;
        }
        if should_rewrite_gemini_stream_to_ndjson(&downstream_request)
            || is_streaming_content_type(response_headers.as_slice())
        {
            let stream_record_context = UpstreamStreamRecordContext {
                state: state.clone(),
                channel: channel.clone(),
                provider: provider.clone(),
                auth,
                request: downstream_request.clone(),
                provider_id,
                credential_id: upstream_credential_id,
                request_meta: upstream_request_meta.clone(),
                response_status: Some(response_status),
                response_headers: response_headers.clone(),
                stream_usage: None,
            };
            return upstream_response_to_axum_stream(
                response,
                should_rewrite_gemini_stream_to_ndjson(&downstream_request),
                Some(stream_record_context),
            );
        }

        let status =
            StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_string(), value.to_string()))
            })
            .collect::<Vec<_>>();
        let body_bytes = response
            .bytes()
            .await
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
        let raw_body = body_bytes.to_vec();
        let normalized_body =
            normalize_upstream_response_body_for_channel(&channel, body_bytes.as_ref())
                .unwrap_or_else(|| raw_body.clone());
        let encoded_for_usage = encode_http_response_for_transform(
            status,
            headers.as_slice(),
            normalized_body.as_ref(),
        )?;
        let usage_source_response = decode_response_for_usage(
            downstream_request.operation(),
            downstream_request.protocol(),
            encoded_for_usage.as_ref(),
        );
        enqueue_upstream_and_usage_event(
            state.as_ref(),
            UpstreamAndUsageEventInput {
                auth,
                request: &downstream_request,
                provider_id,
                credential_id: upstream_credential_id,
                request_meta: upstream_request_meta.as_ref(),
                error_status: None,
                response_status: Some(response_status),
                response_headers: response_headers.as_slice(),
                response_body: Some(raw_body.clone()),
                local_response: usage_source_response.as_ref(),
            },
        )
        .await;
        let mut headers_for_client = headers.clone();
        if normalized_body != raw_body {
            remove_header_ignore_case(&mut headers_for_client, "content-length");
        }
        return response_from_status_headers_and_bytes(
            status,
            headers_for_client.as_slice(),
            normalized_body,
        );
    }

    enqueue_upstream_and_usage_event(
        state.as_ref(),
        UpstreamAndUsageEventInput {
            auth,
            request: &downstream_request,
            provider_id,
            credential_id: upstream_credential_id,
            request_meta: upstream_request_meta.as_ref(),
            error_status: None,
            response_status: None,
            response_headers: &[],
            response_body: None,
            local_response: None,
        },
    )
    .await;
    Err(UpstreamError::UpstreamRequest(
        "upstream returned empty response".to_string(),
    ))
}

async fn execute_transform_candidates(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    candidates: Vec<TransformRequest>,
) -> Result<Response, HttpError> {
    let mut unsupported = false;
    for candidate in candidates {
        match execute_transform_request(
            state.clone(),
            channel.clone(),
            provider.clone(),
            auth,
            candidate,
        )
        .await
        {
            Ok(response) => return Ok(response),
            Err(UpstreamError::UnsupportedRequest) => {
                unsupported = true;
            }
            Err(err) => return Err(HttpError::from(err)),
        }
    }
    if unsupported {
        return Err(HttpError::from(UpstreamError::UnsupportedRequest));
    }
    Err(HttpError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "no provider route candidate executed",
    ))
}

fn normalize_unscoped_model_id(raw: &str) -> String {
    raw.trim()
        .trim_matches('/')
        .strip_prefix("models/")
        .unwrap_or(raw.trim().trim_matches('/'))
        .to_string()
}

fn collect_model_ids_from_response_json(value: &serde_json::Value, out: &mut Vec<String>) {
    if let Some(items) = value.get("data").and_then(serde_json::Value::as_array) {
        for item in items {
            if let Some(id) = item.get("id").and_then(serde_json::Value::as_str) {
                let model_id = normalize_unscoped_model_id(id);
                if !model_id.is_empty() {
                    out.push(model_id);
                }
            }
        }
    }

    if let Some(items) = value.get("models").and_then(serde_json::Value::as_array) {
        for item in items {
            let name = item
                .get("name")
                .and_then(serde_json::Value::as_str)
                .or_else(|| item.get("id").and_then(serde_json::Value::as_str));
            if let Some(name) = name {
                let model_id = normalize_unscoped_model_id(name);
                if !model_id.is_empty() {
                    out.push(model_id);
                }
            }
        }
    }
}

async fn collect_provider_model_ids(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    headers: &HeaderMap,
) -> Vec<String> {
    let channel_prefix = channel.as_str().to_string();
    let mut openai = openai_model_list_request::OpenAiModelListRequest::default();

    let mut claude = claude_model_list_request::ClaudeModelListRequest::default();
    let (version, beta) = anthropic_headers_from_request(headers);
    claude.headers.anthropic_version = version;
    if beta.is_some() {
        claude.headers.anthropic_beta = beta;
    }

    let gemini = gemini_model_list_request::GeminiModelListRequest::default();
    openai.query = openai_model_list_request::QueryParameters::default();

    let response = match execute_transform_candidates(
        state,
        channel,
        provider,
        auth,
        vec![
            TransformRequest::ModelListOpenAi(openai),
            TransformRequest::ModelListClaude(claude),
            TransformRequest::ModelListGemini(gemini),
        ],
    )
    .await
    {
        Ok(response) => response,
        Err(_) => return Vec::new(),
    };

    if !response.status().is_success() {
        return Vec::new();
    }

    let body = match to_bytes(response.into_body(), BODY_CAPTURE_LIMIT_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => return Vec::new(),
    };
    let value = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };

    let mut ids = Vec::new();
    collect_model_ids_from_response_json(&value, &mut ids);
    ids.into_iter()
        .map(|model_id| format!("{channel_prefix}/{model_id}"))
        .collect()
}

async fn collect_unscoped_model_ids(
    state: Arc<AppState>,
    auth: RequestAuthContext,
    headers: &HeaderMap,
) -> Vec<String> {
    let providers: Vec<(ChannelId, ProviderDefinition)> = {
        let snapshot = state.config.load();
        snapshot
            .providers
            .providers
            .iter()
            .map(|provider| (provider.channel.clone(), provider.clone()))
            .collect()
    };

    let mut ids = Vec::new();
    for (channel, provider) in providers {
        ids.extend(
            collect_provider_model_ids(state.clone(), channel, provider, auth, headers).await,
        );
    }

    let mut dedup = std::collections::BTreeSet::new();
    ids.retain(|id| dedup.insert(id.clone()));
    ids
}

async fn oauth_start(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let provider_id = resolve_provider_id(&state, &channel).await.ok();
    let http = if matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode)) {
        state.load_spoof_http()
    } else {
        state.load_http()
    };
    let request = UpstreamOAuthRequest {
        query,
        headers: collect_headers(&headers),
    };
    let response = match provider.execute_oauth_start(http.as_ref(), &request).await {
        Ok(response) => response,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                provider_id,
                None,
                err_request_meta.as_ref(),
                err_status,
                &[],
                None,
            )
            .await;
            return Err(HttpError::from(err));
        }
    };
    enqueue_upstream_request_event_from_meta(
        state.as_ref(),
        provider_id,
        None,
        response.request_meta.as_ref(),
        Some(response.status_code),
        response.headers.as_slice(),
        Some(response.body.clone()),
    )
    .await;
    Ok(oauth_response_to_axum(response))
}

async fn oauth_callback(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let provider_id = resolve_provider_id(&state, &channel).await.ok();
    let http = if matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode)) {
        state.load_spoof_http()
    } else {
        state.load_http()
    };
    let request = UpstreamOAuthRequest {
        query,
        headers: collect_headers(&headers),
    };
    let result = match provider
        .execute_oauth_callback(http.as_ref(), &request)
        .await
    {
        Ok(result) => result,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                provider_id,
                None,
                err_request_meta.as_ref(),
                err_status,
                &[],
                None,
            )
            .await;
            return Err(HttpError::from(err));
        }
    };
    let mut resolved_credential_id: Option<i64> = None;

    if let Some(oauth_credential) = result.credential.as_ref() {
        let provisional = CredentialRef {
            id: -1,
            label: oauth_credential.label.clone(),
            credential: oauth_credential.credential.clone(),
        };
        let provider_id = resolve_provider_id(&state, &channel).await?;
        let credential_id = if let Some(credential_id) =
            parse_optional_query_value::<i64>(request.query.as_deref(), "credential_id")?
        {
            credential_id
        } else {
            resolve_credential_id(&state, provider_id, &provisional).await?
        };
        resolved_credential_id = Some(credential_id);
        let credential_ref = CredentialRef {
            id: credential_id,
            label: oauth_credential.label.clone(),
            credential: oauth_credential.credential.clone(),
        };
        state.upsert_provider_credential_in_memory(&channel, credential_ref.clone());
        persist_provider_and_credential(&state, &channel, &provider, &credential_ref).await?;
    }
    enqueue_upstream_request_event_from_meta(
        state.as_ref(),
        provider_id,
        resolved_credential_id,
        result.response.request_meta.as_ref(),
        Some(result.response.status_code),
        result.response.headers.as_slice(),
        Some(result.response.body.clone()),
    )
    .await;

    Ok(oauth_callback_response_to_axum(
        result,
        resolved_credential_id,
    ))
}

async fn upstream_usage(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let provider_id = resolve_provider_id(&state, &channel).await.ok();
    let http = if matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode)) {
        state.load_spoof_http()
    } else {
        state.load_http()
    };
    let now = now_unix_ms();
    let credential_id = parse_optional_query_value::<i64>(query.as_deref(), "credential_id")?;
    let upstream = match provider
        .execute_upstream_usage_with_retry(
            http.as_ref(),
            &state.credential_states,
            credential_id,
            now,
        )
        .await
    {
        Ok(upstream) => upstream,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                provider_id,
                credential_id,
                err_request_meta.as_ref(),
                err_status,
                &[],
                None,
            )
            .await;
            return Err(HttpError::from(err));
        }
    };
    let upstream_credential_id = upstream.credential_id;
    let upstream_request_meta = upstream.request_meta.clone();

    if let Some(update) = upstream.credential_update.clone() {
        apply_credential_update_and_persist(
            state.clone(),
            channel.clone(),
            provider.clone(),
            update,
        )
        .await;
    }

    let payload = upstream
        .into_http_payload()
        .await
        .map_err(HttpError::from)?;
    enqueue_upstream_request_event_from_meta(
        state.as_ref(),
        provider_id,
        upstream_credential_id,
        upstream_request_meta.as_ref(),
        Some(payload.status_code),
        payload.headers.as_slice(),
        Some(payload.body.clone()),
    )
    .await;
    Ok(oauth_response_to_axum(payload))
}

async fn openai_realtime_upgrade(
    State(state): State<Arc<AppState>>,
    Path(_provider_name): Path<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, headers).await
}

async fn openai_responses_upgrade(
    State(state): State<Arc<AppState>>,
    Path(_provider_name): Path<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, headers).await
}

async fn openai_responses_upgrade_unscoped(
    State(state): State<Arc<AppState>>,
    _query: RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, headers).await
}

async fn openai_realtime_upgrade_with_tail(
    State(state): State<Arc<AppState>>,
    Path((_provider_name, _tail)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, headers).await
}

async fn handle_openai_realtime_upgrade(
    state: Arc<AppState>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    authorize_provider_access(&headers, &state)?;

    Ok(websocket_upgrade_required_response(
        "websocket upstream is not implemented; use /v1/responses (HTTP) for now",
    ))
}

async fn claude_messages(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<claude_create_message_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = claude_create_message_request::ClaudeCreateMessageRequest {
        body,
        ..Default::default()
    };
    let (version, beta) = anthropic_headers_from_request(&headers);
    request.headers.anthropic_version = version;
    if beta.is_some() {
        request.headers.anthropic_beta = beta;
    }
    let envelope = if request.body.stream.unwrap_or(false) {
        TransformRequest::StreamGenerateContentClaude(request)
    } else {
        TransformRequest::GenerateContentClaude(request)
    };
    execute_transform_request(state, channel, provider, auth, envelope)
        .await
        .map_err(HttpError::from)
}

async fn claude_messages_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut body): Json<claude_create_message_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let model = serialize_json_scalar(&body.model, "claude model")?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model.as_str())?;
    body.model = deserialize_json_scalar(stripped_model.as_str(), "claude model")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = claude_create_message_request::ClaudeCreateMessageRequest {
        body,
        ..Default::default()
    };
    let (version, beta) = anthropic_headers_from_request(&headers);
    request.headers.anthropic_version = version;
    if beta.is_some() {
        request.headers.anthropic_beta = beta;
    }
    let envelope = if request.body.stream.unwrap_or(false) {
        TransformRequest::StreamGenerateContentClaude(request)
    } else {
        TransformRequest::GenerateContentClaude(request)
    };
    execute_transform_request(state, channel, provider, auth, envelope)
        .await
        .map_err(HttpError::from)
}

async fn claude_count_tokens(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<claude_count_tokens_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = claude_count_tokens_request::ClaudeCountTokensRequest {
        body,
        ..Default::default()
    };
    let (version, beta) = anthropic_headers_from_request(&headers);
    request.headers.anthropic_version = version;
    if beta.is_some() {
        request.headers.anthropic_beta = beta;
    }
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::CountTokenClaude(request),
    )
    .await
    .map_err(HttpError::from)
}

async fn claude_count_tokens_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut body): Json<claude_count_tokens_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let model = serialize_json_scalar(&body.model, "claude model")?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model.as_str())?;
    body.model = deserialize_json_scalar(stripped_model.as_str(), "claude model")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = claude_count_tokens_request::ClaudeCountTokensRequest {
        body,
        ..Default::default()
    };
    let (version, beta) = anthropic_headers_from_request(&headers);
    request.headers.anthropic_version = version;
    if beta.is_some() {
        request.headers.anthropic_beta = beta;
    }
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::CountTokenClaude(request),
    )
    .await
    .map_err(HttpError::from)
}

async fn openai_chat_completions(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<openai_chat_completions_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let request = openai_chat_completions_request::OpenAiChatCompletionsRequest {
        body,
        ..Default::default()
    };
    let envelope = if request.body.stream.unwrap_or(false) {
        TransformRequest::StreamGenerateContentOpenAiChatCompletions(request)
    } else {
        TransformRequest::GenerateContentOpenAiChatCompletions(request)
    };
    execute_transform_request(state, channel, provider, auth, envelope)
        .await
        .map_err(HttpError::from)
}

async fn openai_chat_completions_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut body): Json<openai_chat_completions_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(body.model.as_str())?;
    body.model = stripped_model;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let request = openai_chat_completions_request::OpenAiChatCompletionsRequest {
        body,
        ..Default::default()
    };
    let envelope = if request.body.stream.unwrap_or(false) {
        TransformRequest::StreamGenerateContentOpenAiChatCompletions(request)
    } else {
        TransformRequest::GenerateContentOpenAiChatCompletions(request)
    };
    execute_transform_request(state, channel, provider, auth, envelope)
        .await
        .map_err(HttpError::from)
}

async fn openai_responses(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<openai_create_response_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let request = openai_create_response_request::OpenAiCreateResponseRequest {
        body,
        ..Default::default()
    };
    let envelope = if request.body.stream.unwrap_or(false) {
        TransformRequest::StreamGenerateContentOpenAiResponse(request)
    } else {
        TransformRequest::GenerateContentOpenAiResponse(request)
    };
    execute_transform_request(state, channel, provider, auth, envelope)
        .await
        .map_err(HttpError::from)
}

async fn openai_responses_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut body): Json<openai_create_response_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let model = body
        .model
        .clone()
        .ok_or_else(|| bad_request("missing `model` in OpenAI responses request body"))?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model.as_str())?;
    body.model = Some(stripped_model);
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let request = openai_create_response_request::OpenAiCreateResponseRequest {
        body,
        ..Default::default()
    };
    let envelope = if request.body.stream.unwrap_or(false) {
        TransformRequest::StreamGenerateContentOpenAiResponse(request)
    } else {
        TransformRequest::GenerateContentOpenAiResponse(request)
    };
    execute_transform_request(state, channel, provider, auth, envelope)
        .await
        .map_err(HttpError::from)
}

async fn openai_input_tokens(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<openai_count_tokens_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let request = openai_count_tokens_request::OpenAiCountTokensRequest {
        body,
        ..Default::default()
    };
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::CountTokenOpenAi(request),
    )
    .await
    .map_err(HttpError::from)
}

async fn openai_input_tokens_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut body): Json<openai_count_tokens_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let model = body
        .model
        .clone()
        .ok_or_else(|| bad_request("missing `model` in OpenAI input_tokens request body"))?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model.as_str())?;
    body.model = Some(stripped_model);
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let request = openai_count_tokens_request::OpenAiCountTokensRequest {
        body,
        ..Default::default()
    };
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::CountTokenOpenAi(request),
    )
    .await
    .map_err(HttpError::from)
}

async fn openai_embeddings(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<openai_embeddings_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let request = openai_embeddings_request::OpenAiEmbeddingsRequest {
        body,
        ..Default::default()
    };
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::EmbeddingOpenAi(request),
    )
    .await
    .map_err(HttpError::from)
}

async fn openai_embeddings_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut body): Json<openai_embeddings_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let model = serialize_json_scalar(&body.model, "openai embeddings model")?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model.as_str())?;
    body.model = deserialize_json_scalar(stripped_model.as_str(), "openai embeddings model")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let request = openai_embeddings_request::OpenAiEmbeddingsRequest {
        body,
        ..Default::default()
    };
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::EmbeddingOpenAi(request),
    )
    .await
    .map_err(HttpError::from)
}

async fn openai_compact(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<openai_compact_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let request = openai_compact_request::OpenAiCompactRequest {
        body,
        ..Default::default()
    };
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::CompactOpenAi(request),
    )
    .await
    .map_err(HttpError::from)
}

async fn openai_compact_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut body): Json<openai_compact_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(body.model.as_str())?;
    body.model = stripped_model;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let request = openai_compact_request::OpenAiCompactRequest {
        body,
        ..Default::default()
    };
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::CompactOpenAi(request),
    )
    .await
    .map_err(HttpError::from)
}

async fn v1_model_list(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;

    let mut openai = openai_model_list_request::OpenAiModelListRequest::default();

    let mut claude = claude_model_list_request::ClaudeModelListRequest::default();
    let (version, beta) = anthropic_headers_from_request(&headers);
    claude.headers.anthropic_version = version;
    if beta.is_some() {
        claude.headers.anthropic_beta = beta;
    }
    claude.query.after_id = parse_query_value(query.as_deref(), "after_id");
    claude.query.before_id = parse_query_value(query.as_deref(), "before_id");
    claude.query.limit = parse_optional_query_value::<u16>(query.as_deref(), "limit")?;

    let mut gemini = gemini_model_list_request::GeminiModelListRequest::default();
    gemini.query.page_size = parse_optional_query_value::<u32>(query.as_deref(), "pageSize")?;
    gemini.query.page_token = parse_query_value(query.as_deref(), "pageToken");

    openai.query = openai_model_list_request::QueryParameters::default();

    execute_transform_candidates(
        state,
        channel,
        provider,
        auth,
        vec![
            TransformRequest::ModelListOpenAi(openai),
            TransformRequest::ModelListClaude(claude),
            TransformRequest::ModelListGemini(gemini),
        ],
    )
    .await
}

async fn v1_model_list_unscoped(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let ids = collect_unscoped_model_ids(state, auth, &headers).await;
    let data = ids
        .into_iter()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "created": 0,
                "owned_by": "GPROXY",
            })
        })
        .collect::<Vec<_>>();
    let body = serde_json::to_vec(&json!({
        "object": "list",
        "data": data,
    }))
    .map_err(|err| internal_error(format!("serialize model list response failed: {err}")))?;
    response_from_status_headers_and_bytes(
        StatusCode::OK,
        &[("content-type".to_string(), "application/json".to_string())],
        body,
    )
    .map_err(HttpError::from)
}

async fn v1_model_get(
    State(state): State<Arc<AppState>>,
    Path((provider_name, model_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;

    let mut openai = openai_model_get_request::OpenAiModelGetRequest::default();
    openai.path.model = model_id.clone();

    let mut claude = claude_model_get_request::ClaudeModelGetRequest::default();
    let (version, beta) = anthropic_headers_from_request(&headers);
    claude.headers.anthropic_version = version;
    if beta.is_some() {
        claude.headers.anthropic_beta = beta;
    }
    claude.path.model_id = model_id.clone();

    let mut gemini = gemini_model_get_request::GeminiModelGetRequest::default();
    gemini.path.name = normalize_gemini_model_path(model_id.as_str())?;

    execute_transform_candidates(
        state,
        channel,
        provider,
        auth,
        vec![
            TransformRequest::ModelGetOpenAi(openai),
            TransformRequest::ModelGetClaude(claude),
            TransformRequest::ModelGetGemini(gemini),
        ],
    )
    .await
}

async fn v1_model_get_unscoped(
    State(state): State<Arc<AppState>>,
    Path(model_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (provider_name, stripped_model_id) =
        split_provider_prefixed_plain_model(model_id.as_str())?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;

    let mut openai = openai_model_get_request::OpenAiModelGetRequest::default();
    openai.path.model = stripped_model_id.clone();

    let mut claude = claude_model_get_request::ClaudeModelGetRequest::default();
    let (version, beta) = anthropic_headers_from_request(&headers);
    claude.headers.anthropic_version = version;
    if beta.is_some() {
        claude.headers.anthropic_beta = beta;
    }
    claude.path.model_id = stripped_model_id.clone();

    let mut gemini = gemini_model_get_request::GeminiModelGetRequest::default();
    gemini.path.name = normalize_gemini_model_path(stripped_model_id.as_str())?;

    execute_transform_candidates(
        state,
        channel,
        provider,
        auth,
        vec![
            TransformRequest::ModelGetOpenAi(openai),
            TransformRequest::ModelGetClaude(claude),
            TransformRequest::ModelGetGemini(gemini),
        ],
    )
    .await
}

async fn v1beta_model_list(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = gemini_model_list_request::GeminiModelListRequest::default();
    request.query.page_size = parse_optional_query_value::<u32>(query.as_deref(), "pageSize")?;
    request.query.page_token = parse_query_value(query.as_deref(), "pageToken");
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::ModelListGemini(request),
    )
    .await
    .map_err(HttpError::from)
}

async fn v1beta_model_list_unscoped(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let ids = collect_unscoped_model_ids(state, auth, &headers).await;
    let models = ids
        .into_iter()
        .map(|id| {
            json!({
                "name": format!("models/{id}"),
                "displayName": id,
            })
        })
        .collect::<Vec<_>>();
    let body = serde_json::to_vec(&json!({
        "models": models,
    }))
    .map_err(|err| internal_error(format!("serialize model list response failed: {err}")))?;
    response_from_status_headers_and_bytes(
        StatusCode::OK,
        &[("content-type".to_string(), "application/json".to_string())],
        body,
    )
    .map_err(HttpError::from)
}

async fn v1beta_model_get(
    State(state): State<Arc<AppState>>,
    Path((provider_name, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = gemini_model_get_request::GeminiModelGetRequest::default();
    request.path.name = normalize_gemini_model_path(name.as_str())?;
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::ModelGetGemini(request),
    )
    .await
    .map_err(HttpError::from)
}

async fn v1beta_model_get_unscoped(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (provider_name, stripped_name) = split_provider_prefixed_model_path(name.as_str())?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = gemini_model_get_request::GeminiModelGetRequest::default();
    request.path.name = normalize_gemini_model_path(stripped_name.as_str())?;
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::ModelGetGemini(request),
    )
    .await
    .map_err(HttpError::from)
}

async fn v1beta_post_target(
    State(state): State<Arc<AppState>>,
    Path((provider_name, target)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    handle_gemini_post_target(state, provider_name, target, query, headers, body).await
}

async fn v1beta_post_target_unscoped(
    State(state): State<Arc<AppState>>,
    Path(target): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let (provider_name, stripped_target) = split_provider_prefixed_gemini_target(target.as_str())?;
    handle_gemini_post_target(state, provider_name, stripped_target, query, headers, body).await
}

async fn v1_post_target(
    State(state): State<Arc<AppState>>,
    Path((provider_name, target)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    handle_gemini_post_target(state, provider_name, target, query, headers, body).await
}

async fn v1_post_target_unscoped(
    State(state): State<Arc<AppState>>,
    Path(target): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let (provider_name, stripped_target) = split_provider_prefixed_gemini_target(target.as_str())?;
    handle_gemini_post_target(state, provider_name, stripped_target, query, headers, body).await
}

async fn handle_gemini_post_target(
    state: Arc<AppState>,
    provider_name: String,
    target: String,
    query: Option<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;

    if let Some(model) = target.strip_suffix(":generateContent") {
        let normalized_model = normalize_gemini_model_path(model)?;
        let request_body = parse_json_body::<gemini_generate_content_request::RequestBody>(
            &body,
            "invalid gemini generateContent request body",
        )?;
        let mut request = gemini_generate_content_request::GeminiGenerateContentRequest::default();
        request.path.model = normalized_model;
        request.body = request_body;
        return execute_transform_request(
            state,
            channel,
            provider,
            auth,
            TransformRequest::GenerateContentGemini(request),
        )
        .await
        .map_err(HttpError::from);
    }

    if let Some(model) = target.strip_suffix(":streamGenerateContent") {
        let normalized_model = normalize_gemini_model_path(model)?;
        let request_body = parse_json_body::<gemini_stream_generate_content_request::RequestBody>(
            &body,
            "invalid gemini streamGenerateContent request body",
        )?;
        let mut request =
            gemini_stream_generate_content_request::GeminiStreamGenerateContentRequest::default();
        request.path.model = normalized_model;
        request.body = request_body;

        let alt = parse_query_value(query.as_deref(), "alt");
        let envelope = match alt.as_deref() {
            Some("sse") | Some("SSE") => {
                request.query.alt =
                    Some(gemini_stream_generate_content_request::AltQueryParameter::Sse);
                TransformRequest::StreamGenerateContentGeminiSse(request)
            }
            Some(other) => {
                return Err(bad_request(format!(
                    "unsupported gemini stream `alt` query parameter: {other}"
                )));
            }
            None => TransformRequest::StreamGenerateContentGeminiNdjson(request),
        };

        return execute_transform_request(state, channel, provider, auth, envelope)
            .await
            .map_err(HttpError::from);
    }

    if let Some(model) = target.strip_suffix(":countTokens") {
        let normalized_model = normalize_gemini_model_path(model)?;
        let request_body = parse_json_body::<gemini_count_tokens_request::RequestBody>(
            &body,
            "invalid gemini countTokens request body",
        )?;
        let mut request = gemini_count_tokens_request::GeminiCountTokensRequest::default();
        request.path.model = normalized_model;
        request.body = request_body;
        return execute_transform_request(
            state,
            channel,
            provider,
            auth,
            TransformRequest::CountTokenGemini(request),
        )
        .await
        .map_err(HttpError::from);
    }

    if let Some(model) = target.strip_suffix(":embedContent") {
        let normalized_model = normalize_gemini_model_path(model)?;
        let request_body = parse_json_body::<gemini_embeddings_request::RequestBody>(
            &body,
            "invalid gemini embedContent request body",
        )?;
        let mut request = gemini_embeddings_request::GeminiEmbedContentRequest::default();
        request.path.model = normalized_model;
        request.body = request_body;
        return execute_transform_request(
            state,
            channel,
            provider,
            auth,
            TransformRequest::EmbeddingGemini(request),
        )
        .await
        .map_err(HttpError::from);
    }

    Err(HttpError::new(
        StatusCode::NOT_FOUND,
        format!("unsupported gemini endpoint target: {target}"),
    ))
}
