use std::collections::BTreeMap;

use async_stream::try_stream;
use bytes::Bytes;
use futures_util::StreamExt;
use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequest};
use http::{Response as HttpResponse, StatusCode};
use serde_json::{Map, Value, json};
use url::Url;
use wreq::{Body as WreqBody, Client as WreqClient, Method as WreqMethod};

use super::constants::{APP_CHAT_PATH, DEFAULT_USER_AGENT};
use super::settings::GrokSettings;
use crate::channels::retry::{
    CacheAffinityProtocol, CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    configured_pick_mode_uses_cache, credential_pick_mode,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{
    UpstreamError, UpstreamResponse, add_or_replace_header, extra_headers_from_payload_value,
    extra_headers_from_transform_request, merge_extra_headers, payload_body_value,
};
use crate::channels::utils::{join_base_url_and_path, retry_after_to_millis, to_wreq_method};
use crate::channels::{
    BuiltinChannelCredential, BuiltinChannelSettings, ChannelCredential, ChannelSettings,
};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::{ProviderDefinition, RetryWithPayloadRequest};

mod cf;
mod image;
mod models;
mod prepared;
mod response;
mod stream;
mod upload;
mod video;
mod web;

use self::image::execute_grok_image_with_retry;
use self::models::{build_model_get_http_response, build_model_list_http_response};
use self::response::{
    build_nonstream_http_response, build_openai_error_http_response, build_stream_http_response,
    local_http_response,
};
use self::cf::{invalidate_grok_session, resolve_grok_session};
use self::video::{
    execute_grok_video_content_get_with_retry, execute_grok_video_create_with_retry,
    execute_grok_video_get,
};
use self::web::{build_grok_web_headers, build_grok_web_payload};

const GROK_ACCEPT_LANGUAGE: &str = "zh-CN,zh;q=0.9,en;q=0.8";
const GROK_BAGGAGE: &str = "sentry-environment=production,sentry-release=d6add6fb0460641fd482d767a335ef72b9b6abb8,sentry-public_key=b311e0f2690c81f25e2c4cf6d4f7ce1c";
const GROK_STATIC_STATSIG_ID: &str = "ZTpUeXBlRXJyb3I6IENhbm5vdCByZWFkIHByb3BlcnRpZXMgb2YgdW5kZWZpbmVkIChyZWFkaW5nICdjaGlsZE5vZGVzJyk=";

#[derive(Debug, Clone)]
enum GrokPreparedRequest {
    ModelList,
    ModelGet { target: String },
    Chat(GrokPreparedChatRequest),
    Image(GrokPreparedImageRequest),
    VideoCreate(GrokPreparedVideoCreateRequest),
    VideoGet { video_id: String },
    VideoContentGet(GrokPreparedVideoContentRequest),
}

#[derive(Debug, Clone)]
struct GrokPreparedChatRequest {
    stream: bool,
    request_model: String,
    resolved_model: GrokResolvedModel,
    extra_headers: Vec<(String, String)>,
    prompt: String,
    tool_names: Vec<String>,
    cache_body: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrokImageResponseFormat {
    B64Json,
    Url,
}

#[derive(Debug, Clone)]
struct GrokPreparedImageRequest {
    stream: bool,
    request_model: String,
    extra_headers: Vec<(String, String)>,
    prompt: String,
    n: u32,
    aspect_ratio: String,
    request_size: String,
    response_format: GrokImageResponseFormat,
    output_format: String,
    quality: String,
    background: String,
}

#[derive(Debug, Clone)]
struct GrokPreparedVideoCreateRequest {
    request_model: String,
    extra_headers: Vec<(String, String)>,
    prompt: String,
    reference_url: Option<String>,
    aspect_ratio: String,
    size: String,
    seconds: String,
    video_length: u32,
    resolution_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrokPreparedVideoContentVariant {
    Video,
    Thumbnail,
    Spritesheet,
}

#[derive(Debug, Clone)]
struct GrokPreparedVideoContentRequest {
    video_id: String,
    variant: GrokPreparedVideoContentVariant,
}

#[derive(Debug, Clone)]
struct GrokResolvedModel {
    request_model: String,
    upstream_model: String,
    upstream_mode: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct GrokTooling {
    prompt_prefix: Option<String>,
    tool_names: Vec<String>,
}

#[derive(Debug, Clone)]
struct GrokFunctionTool {
    name: String,
    description: Option<String>,
    parameters: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrokToolChoiceMode {
    Auto,
    None,
    Required,
}

#[derive(Debug, Clone)]
struct GrokToolCall {
    id: String,
    name: String,
    arguments: String,
    index: u32,
}

#[derive(Debug, Clone)]
enum GrokToolStreamEvent {
    Text(String),
    Tool(GrokToolCall),
}

#[derive(Debug, Clone, Default)]
struct GrokToolCallStreamState {
    enabled: bool,
    allowed_names: Vec<String>,
    state: GrokToolParserState,
    tool_buffer: String,
    partial: String,
    next_index: u32,
    saw_tool_call: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum GrokToolParserState {
    #[default]
    Text,
    Tool,
}

#[derive(Debug, Clone)]
enum OpenAiChatSseEvent {
    Chunk(Value),
    Done,
}

#[derive(Debug, Clone)]
struct GrokLineStreamParser {
    model: String,
    tool_stream: GrokToolCallStreamState,
    response_id: String,
    created: u64,
    fingerprint: Option<String>,
    saw_chunk: bool,
    saw_visible_output: bool,
    final_message: Option<String>,
    final_message_emitted: bool,
}

#[derive(Debug, Clone, Default)]
struct ChatCompletionAccumulator {
    id: Option<String>,
    created: Option<u64>,
    model: Option<String>,
    system_fingerprint: Option<String>,
    content: String,
    reasoning_content: String,
    tool_calls: BTreeMap<u32, ToolCallAccumulator>,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ToolCallAccumulator {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
    type_: Option<String>,
}

pub async fn execute_grok_with_retry(
    default_client: &WreqClient,
    spoof_client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared = GrokPreparedRequest::from_transform_request(request)?;
    let client = select_grok_client(default_client, spoof_client);
    execute_grok_with_prepared(client, provider, credential_states, prepared, now_unix_ms).await
}

pub async fn execute_grok_payload_with_retry(
    default_client: &WreqClient,
    spoof_client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    payload: RetryWithPayloadRequest<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared =
        GrokPreparedRequest::from_payload(payload.operation, payload.protocol, payload.body)?;
    let client = select_grok_client(default_client, spoof_client);
    execute_grok_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        payload.now_unix_ms,
    )
    .await
}

async fn execute_grok_with_prepared(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: GrokPreparedRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    match prepared {
        GrokPreparedRequest::ModelList => {
            Ok(local_http_response(build_model_list_http_response()?))
        }
        GrokPreparedRequest::ModelGet { target } => Ok(local_http_response(
            build_model_get_http_response(target.as_str())?,
        )),
        GrokPreparedRequest::Chat(prepared) => {
            execute_grok_chat_with_retry(client, provider, credential_states, prepared, now_unix_ms)
                .await
        }
        GrokPreparedRequest::Image(prepared) => {
            execute_grok_image_with_retry(
                client,
                provider,
                credential_states,
                prepared,
                now_unix_ms,
            )
            .await
        }
        GrokPreparedRequest::VideoCreate(prepared) => {
            execute_grok_video_create_with_retry(
                client,
                provider,
                credential_states,
                prepared,
                now_unix_ms,
            )
            .await
        }
        GrokPreparedRequest::VideoGet { video_id } => execute_grok_video_get(video_id.as_str()),
        GrokPreparedRequest::VideoContentGet(prepared) => {
            execute_grok_video_content_get_with_retry(
                client,
                provider,
                credential_states,
                prepared,
                now_unix_ms,
            )
            .await
        }
    }
}

async fn execute_grok_chat_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: GrokPreparedChatRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }

    let settings = grok_settings(provider);
    let url = join_base_url_and_path(base_url, APP_CHAT_PATH);
    let state_manager = CredentialStateManager::new(now_unix_ms);
    let model_for_selection = Some(prepared.request_model.clone());
    let resolved_model_template = prepared.resolved_model.clone();
    let prompt_template = prepared.prompt.clone();
    let extra_headers_template = prepared.extra_headers.clone();
    let cache_body_template = prepared.cache_body.clone();
    let tool_names_template = prepared.tool_names.clone();
    let settings_template = settings.clone();
    let url_template = url.clone();
    let stream = prepared.stream;

    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        cache_affinity_hint_from_transform_request(
            CacheAffinityProtocol::OpenAiChatCompletions,
            model_for_selection.as_deref(),
            Some(cache_body_template.as_slice()),
        )
    } else {
        None
    };
    let pick_mode =
        credential_pick_mode(provider.credential_pick_mode, cache_affinity_hint.as_ref());

    retry_with_eligible_credentials_with_affinity(
        crate::channels::retry::CredentialRetryContext {
            provider,
            credential_states,
            model: model_for_selection.as_deref(),
            now_unix_ms,
            pick_mode,
            cache_affinity_hint,
        },
        |credential| match &credential.credential {
            ChannelCredential::Builtin(BuiltinChannelCredential::Grok(value)) => {
                Some(value.clone())
            }
            _ => None,
        },
        |attempt| {
            let resolved_model = resolved_model_template.clone();
            let prompt = prompt_template.clone();
            let extra_headers = extra_headers_template.clone();
            let tool_names = tool_names_template.clone();
            let settings = settings_template.clone();
            let url = url_template.clone();
            let model = model_for_selection.clone();

            async move {
                let request_body =
                    match build_grok_web_payload(prompt.as_str(), &settings, &resolved_model) {
                        Ok(value) => value,
                        Err(err) => {
                            return CredentialRetryDecision::Retry {
                                last_status: None,
                                last_error: Some(err.to_string()),
                                last_request_meta: None,
                            };
                        }
                    };
                let session = match resolve_grok_session(
                    client,
                    &settings,
                    base_url,
                    attempt.material.sso.as_str(),
                )
                .await
                {
                    Ok(value) => value,
                    Err(err) => {
                        return CredentialRetryDecision::Retry {
                            last_status: None,
                            last_error: Some(err.to_string()),
                            last_request_meta: None,
                        };
                    }
                };
                let sent_headers = match build_grok_web_headers(
                    session.user_agent.as_deref().or(settings.user_agent.as_deref()),
                    session.extra_cookie_header.as_deref(),
                    attempt.material.sso.as_str(),
                    extra_headers.as_slice(),
                    base_url,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        return CredentialRetryDecision::Retry {
                            last_status: None,
                            last_error: Some(err.to_string()),
                            last_request_meta: None,
                        };
                    }
                };
                let send = crate::channels::upstream::tracked_send_request(
                    client,
                    WreqMethod::POST,
                    url.as_str(),
                    sent_headers,
                    Some(request_body),
                )
                .await;
                match send {
                    Ok((response, request_meta)) => {
                        let status = response.status();
                        let status_code = status.as_u16();

                        if status.is_success() {
                            let converted = if stream {
                                match build_stream_http_response(
                                    response,
                                    resolved_model.request_model.clone(),
                                    tool_names,
                                ) {
                                    Ok(value) => value,
                                    Err(err) => {
                                        return CredentialRetryDecision::Retry {
                                            last_status: Some(status_code),
                                            last_error: Some(err.to_string()),
                                            last_request_meta: None,
                                        };
                                    }
                                }
                            } else {
                                match build_nonstream_http_response(
                                    response,
                                    resolved_model.request_model.clone(),
                                    tool_names,
                                )
                                .await
                                {
                                    Ok(value) => value,
                                    Err(err) => {
                                        return CredentialRetryDecision::Retry {
                                            last_status: Some(status_code),
                                            last_error: Some(err.to_string()),
                                            last_request_meta: None,
                                        };
                                    }
                                }
                            };

                            state_manager.mark_success(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                            );
                            return CredentialRetryDecision::Return(
                                UpstreamResponse::from_http(
                                    attempt.credential_id,
                                    attempt.attempts,
                                    converted,
                                )
                                .with_request_meta(request_meta),
                            );
                        }

                        if status_code == 401 {
                            let message = format!("upstream status {status_code}");
                            state_manager.mark_auth_dead(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                                Some(message.clone()),
                            );
                            return CredentialRetryDecision::Retry {
                                last_status: Some(status_code),
                                last_error: Some(message),
                                last_request_meta: None,
                            };
                        }

                        if status_code == 429 {
                            let retry_after_ms = retry_after_to_millis(response.headers());
                            let message = format!("upstream status {status_code}");
                            state_manager.mark_rate_limited(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                                model.as_deref(),
                                retry_after_ms,
                                Some(message.clone()),
                            );
                            return CredentialRetryDecision::Retry {
                                last_status: Some(status_code),
                                last_error: Some(message),
                                last_request_meta: None,
                            };
                        }

                        if status_code == 403 || matches!(status_code, 500 | 502 | 503 | 504) {
                            if status_code == 403 {
                                invalidate_grok_session(
                                    &settings,
                                    base_url,
                                    attempt.material.sso.as_str(),
                                );
                            }
                            let message = format!("upstream status {status_code}");
                            state_manager.mark_transient_failure(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                                model.as_deref(),
                                None,
                                Some(message.clone()),
                            );
                            return CredentialRetryDecision::Retry {
                                last_status: Some(status_code),
                                last_error: Some(message),
                                last_request_meta: None,
                            };
                        }

                        match build_openai_error_http_response(response).await {
                            Ok(converted) => CredentialRetryDecision::Return(
                                UpstreamResponse::from_http(
                                    attempt.credential_id,
                                    attempt.attempts,
                                    converted,
                                )
                                .with_request_meta(request_meta),
                            ),
                            Err(err) => {
                                let message = err.to_string();
                                state_manager.mark_transient_failure(
                                    credential_states,
                                    &provider.channel,
                                    attempt.credential_id,
                                    model.as_deref(),
                                    None,
                                    Some(message.clone()),
                                );
                                CredentialRetryDecision::Retry {
                                    last_status: Some(status_code),
                                    last_error: Some(message),
                                    last_request_meta: None,
                                }
                            }
                        }
                    }
                    Err(err) => {
                        let message = err.to_string();
                        state_manager.mark_transient_failure(
                            credential_states,
                            &provider.channel,
                            attempt.credential_id,
                            model.as_deref(),
                            None,
                            Some(message.clone()),
                        );
                        CredentialRetryDecision::Retry {
                            last_status: None,
                            last_error: Some(message),
                            last_request_meta: None,
                        }
                    }
                }
            }
        },
    )
    .await
}

fn select_grok_client<'a>(
    default_client: &'a WreqClient,
    spoof_client: &'a WreqClient,
) -> &'a WreqClient {
    let _ = default_client;
    spoof_client
}

fn grok_settings(provider: &ProviderDefinition) -> GrokSettings {
    match &provider.settings {
        ChannelSettings::Builtin(BuiltinChannelSettings::Grok(value)) => value.clone(),
        _ => GrokSettings::default(),
    }
}
