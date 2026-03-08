use std::sync::LazyLock;

use dashmap::DashMap;
use futures_util::StreamExt as _;
use http::StatusCode;
use serde_json::{Value, json};
use wreq::Method as WreqMethod;

use crate::channels::retry::{
    CredentialRetryDecision, credential_pick_mode, retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::tracked_send_request;
use crate::channels::{
    BuiltinChannelCredential, BuiltinChannelSettings, ChannelCredential, ChannelSettings,
};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;

use super::response::{
    build_json_http_response, build_openai_error_http_response, build_openai_error_json_response,
    local_http_response,
};
use super::stream::{next_line, random_hex, unix_timestamp_secs};
use super::upload::{build_grok_upload_file_body, extract_uploaded_asset_url};
use super::web::{
    build_grok_download_headers, build_grok_media_post_payload, build_grok_video_payload,
    build_grok_web_headers,
};
use super::*;
use super::cf::{invalidate_grok_session, resolve_grok_session};

static VIDEO_STORE: LazyLock<DashMap<String, GrokStoredVideo>> =
    LazyLock::new(DashMap::<String, GrokStoredVideo>::default);

#[derive(Debug, Clone)]
struct GrokStoredVideo {
    id: String,
    created_at: u64,
    completed_at: u64,
    model: String,
    progress: f64,
    prompt: String,
    seconds: String,
    size: String,
    status: &'static str,
    thumbnail_url: Option<String>,
    video_url: String,
}

#[derive(Debug, Clone, Default)]
struct GrokVideoRunResult {
    response_id: Option<String>,
    thumbnail_url: Option<String>,
    video_url: Option<String>,
}

pub(super) async fn execute_grok_video_create_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: GrokPreparedVideoCreateRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }

    let settings = video_settings(provider);
    let media_post_url =
        join_base_url_and_path(base_url, super::super::constants::MEDIA_POST_CREATE_PATH);
    let upload_file_url =
        join_base_url_and_path(base_url, super::super::constants::UPLOAD_FILE_PATH);
    let app_chat_url = join_base_url_and_path(base_url, super::super::constants::APP_CHAT_PATH);
    let state_manager = CredentialStateManager::new(now_unix_ms);
    let model_hint = Some(prepared.request_model.clone());
    let pick_mode = credential_pick_mode(provider.credential_pick_mode, None);

    retry_with_eligible_credentials_with_affinity(
        crate::channels::retry::CredentialRetryContext {
            provider,
            credential_states,
            model: model_hint.as_deref(),
            now_unix_ms,
            pick_mode,
            cache_affinity_hint: None,
        },
        |credential| match &credential.credential {
            ChannelCredential::Builtin(BuiltinChannelCredential::Grok(value)) => {
                Some(value.clone())
            }
            _ => None,
        },
        |attempt| {
            let settings = settings.clone();
            let prepared = prepared.clone();
            let media_post_url = media_post_url.clone();
            let upload_file_url = upload_file_url.clone();
            let app_chat_url = app_chat_url.clone();
            let model_hint = model_hint.clone();

            async move {
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
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            None,
                            err.to_string(),
                            None,
                        );
                    }
                };
                let headers = match build_grok_web_headers(
                    session.user_agent.as_deref().or(settings.user_agent.as_deref()),
                    session.extra_cookie_header.as_deref(),
                    attempt.material.sso.as_str(),
                    prepared.extra_headers.as_slice(),
                    base_url,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            None,
                            err.to_string(),
                            None,
                        );
                    }
                };

                let reference_url = if let Some(reference_url) = prepared.reference_url.as_deref() {
                    if reference_url.starts_with("data:") {
                        let upload_body = match build_grok_upload_file_body(reference_url) {
                            Ok(value) => value,
                            Err(err) => {
                                return retry_video_failure(
                                    provider,
                                    credential_states,
                                    &state_manager,
                                    attempt.credential_id,
                                    model_hint.as_deref(),
                                    None,
                                    err,
                                    None,
                                );
                            }
                        };
                        let (upload_response, upload_request_meta) = match tracked_send_request(
                            client,
                            WreqMethod::POST,
                            upload_file_url.as_str(),
                            headers.clone(),
                            Some(upload_body),
                        )
                        .await
                        {
                            Ok(value) => value,
                            Err(err) => {
                                return retry_video_failure(
                                    provider,
                                    credential_states,
                                    &state_manager,
                                    attempt.credential_id,
                                    model_hint.as_deref(),
                                    err.status().map(|status| status.as_u16()),
                                    err.to_string(),
                                    None,
                                );
                            }
                        };
                        let upload_status = upload_response.status();
                        if !upload_status.is_success() {
                            return retry_or_return_video_http_error(
                                provider,
                                credential_states,
                                &state_manager,
                                &settings,
                                base_url,
                                attempt.material.sso.as_str(),
                                attempt.credential_id,
                                attempt.attempts,
                                model_hint.as_deref(),
                                upload_response,
                                Some(upload_request_meta),
                            )
                            .await;
                        }
                        let upload_request_meta_for_error = upload_request_meta.clone();
                        let asset_url = match extract_uploaded_asset_url(upload_response).await {
                            Ok(value) => value,
                            Err(err) => {
                                return retry_video_failure(
                                    provider,
                                    credential_states,
                                    &state_manager,
                                    attempt.credential_id,
                                    model_hint.as_deref(),
                                    None,
                                    err,
                                    Some(upload_request_meta_for_error),
                                );
                            }
                        };
                        Some(asset_url)
                    } else {
                        Some(reference_url.to_string())
                    }
                } else {
                    None
                };

                let media_post_body = match build_grok_media_post_payload(
                    prepared.prompt.as_str(),
                    reference_url.as_deref(),
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            None,
                            err.to_string(),
                            None,
                        );
                    }
                };

                let (media_post_response, media_post_request_meta) = match tracked_send_request(
                    client,
                    WreqMethod::POST,
                    media_post_url.as_str(),
                    headers.clone(),
                    Some(media_post_body),
                )
                .await
                {
                    Ok(value) => value,
                    Err(err) => {
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            err.status().map(|status| status.as_u16()),
                            err.to_string(),
                            None,
                        );
                    }
                };

                let media_post_status = media_post_response.status();
                if !media_post_status.is_success() {
                    return retry_or_return_video_http_error(
                        provider,
                        credential_states,
                        &state_manager,
                        &settings,
                        base_url,
                        attempt.material.sso.as_str(),
                        attempt.credential_id,
                        attempt.attempts,
                        model_hint.as_deref(),
                        media_post_response,
                        Some(media_post_request_meta),
                    )
                    .await;
                }

                let post_id = match extract_media_post_id(media_post_response).await {
                    Ok(value) => value,
                    Err(err) => {
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            None,
                            err,
                            None,
                        );
                    }
                };

                let payload = match build_grok_video_payload(
                    prepared.prompt.as_str(),
                    post_id.as_str(),
                    prepared.aspect_ratio.as_str(),
                    prepared.resolution_name.as_str(),
                    prepared.video_length,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            None,
                            err.to_string(),
                            None,
                        );
                    }
                };

                let (app_chat_response, request_meta) = match tracked_send_request(
                    client,
                    WreqMethod::POST,
                    app_chat_url.as_str(),
                    headers,
                    Some(payload),
                )
                .await
                {
                    Ok(value) => value,
                    Err(err) => {
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            err.status().map(|status| status.as_u16()),
                            err.to_string(),
                            None,
                        );
                    }
                };

                let app_chat_status = app_chat_response.status();
                if !app_chat_status.is_success() {
                    return retry_or_return_video_http_error(
                        provider,
                        credential_states,
                        &state_manager,
                        &settings,
                        base_url,
                        attempt.material.sso.as_str(),
                        attempt.credential_id,
                        attempt.attempts,
                        model_hint.as_deref(),
                        app_chat_response,
                        Some(request_meta),
                    )
                    .await;
                }

                let result = match collect_video_run_result(app_chat_response).await {
                    Ok(result) => result,
                    Err(err) => {
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            None,
                            err,
                            Some(request_meta),
                        );
                    }
                };
                let Some(video_url) = result
                    .video_url
                    .as_deref()
                    .filter(|value| !value.is_empty())
                else {
                    return retry_video_failure(
                        provider,
                        credential_states,
                        &state_manager,
                        attempt.credential_id,
                        model_hint.as_deref(),
                        None,
                        "grok video generation returned no final video url".to_string(),
                        Some(request_meta),
                    );
                };

                let now = unix_timestamp_secs();
                let id = format!("vid_{}", random_hex(24));
                let stored = GrokStoredVideo {
                    id: id.clone(),
                    created_at: now,
                    completed_at: now,
                    model: prepared.request_model.clone(),
                    progress: 100.0,
                    prompt: prepared.prompt.clone(),
                    seconds: prepared.seconds.clone(),
                    size: prepared.size.clone(),
                    status: "completed",
                    thumbnail_url: result.thumbnail_url.clone(),
                    video_url: video_url.to_string(),
                };
                VIDEO_STORE.insert(id.clone(), stored.clone());

                let response = match build_json_http_response(StatusCode::OK, &stored.to_json()) {
                    Ok(response) => response,
                    Err(err) => {
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            Some(StatusCode::BAD_GATEWAY.as_u16()),
                            err.to_string(),
                            Some(request_meta),
                        );
                    }
                };
                state_manager.mark_success(
                    credential_states,
                    &provider.channel,
                    attempt.credential_id,
                );
                CredentialRetryDecision::Return(
                    UpstreamResponse::from_http(attempt.credential_id, attempt.attempts, response)
                        .with_request_meta(request_meta),
                )
            }
        },
    )
    .await
}

pub(super) fn execute_grok_video_get(video_id: &str) -> Result<UpstreamResponse, UpstreamError> {
    let Some(video) = VIDEO_STORE.get(video_id).map(|entry| entry.clone()) else {
        return Ok(local_http_response(build_openai_error_json_response(
            StatusCode::NOT_FOUND,
            format!("video '{video_id}' not found"),
            "invalid_request_error",
            Some("video_id"),
            Some("not_found"),
        )?));
    };
    Ok(local_http_response(build_json_http_response(
        StatusCode::OK,
        &video.to_json(),
    )?))
}

pub(super) async fn execute_grok_video_content_get_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: GrokPreparedVideoContentRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }
    let Some(video) = VIDEO_STORE
        .get(prepared.video_id.as_str())
        .map(|entry| entry.clone())
    else {
        return Ok(local_http_response(build_openai_error_json_response(
            StatusCode::NOT_FOUND,
            format!("video '{}' not found", prepared.video_id),
            "invalid_request_error",
            Some("video_id"),
            Some("not_found"),
        )?));
    };
    let target_url = match prepared.variant {
        GrokPreparedVideoContentVariant::Video => video.video_url.clone(),
        GrokPreparedVideoContentVariant::Thumbnail => {
            let Some(url) = video.thumbnail_url.clone() else {
                return Ok(local_http_response(build_openai_error_json_response(
                    StatusCode::NOT_FOUND,
                    format!("video '{}' has no thumbnail", prepared.video_id),
                    "invalid_request_error",
                    Some("variant"),
                    Some("not_found"),
                )?));
            };
            url
        }
        GrokPreparedVideoContentVariant::Spritesheet => {
            return Ok(local_http_response(build_openai_error_json_response(
                StatusCode::NOT_FOUND,
                "grok-web video does not provide spritesheet content",
                "invalid_request_error",
                Some("variant"),
                Some("not_supported"),
            )?));
        }
    };

    let settings = video_settings(provider);
    let state_manager = CredentialStateManager::new(now_unix_ms);
    let model_hint = Some(video.model.clone());
    let pick_mode = credential_pick_mode(provider.credential_pick_mode, None);

    retry_with_eligible_credentials_with_affinity(
        crate::channels::retry::CredentialRetryContext {
            provider,
            credential_states,
            model: model_hint.as_deref(),
            now_unix_ms,
            pick_mode,
            cache_affinity_hint: None,
        },
        |credential| match &credential.credential {
            ChannelCredential::Builtin(BuiltinChannelCredential::Grok(value)) => {
                Some(value.clone())
            }
            _ => None,
        },
        |attempt| {
            let settings = settings.clone();
            let model_hint = model_hint.clone();
            let target_url = target_url.clone();
            async move {
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
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            None,
                            err.to_string(),
                            None,
                        );
                    }
                };
                let headers = match build_grok_download_headers(
                    session.user_agent.as_deref().or(settings.user_agent.as_deref()),
                    session.extra_cookie_header.as_deref(),
                    attempt.material.sso.as_str(),
                    &[],
                    base_url,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            None,
                            err.to_string(),
                            None,
                        );
                    }
                };

                let (response, request_meta) = match tracked_send_request(
                    client,
                    WreqMethod::GET,
                    target_url.as_str(),
                    headers,
                    None,
                )
                .await
                {
                    Ok(value) => value,
                    Err(err) => {
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            err.status().map(|status| status.as_u16()),
                            err.to_string(),
                            None,
                        );
                    }
                };

                let status = response.status();
                if status.is_success() {
                    state_manager.mark_success(
                        credential_states,
                        &provider.channel,
                        attempt.credential_id,
                    );
                    return CredentialRetryDecision::Return(
                        UpstreamResponse::from_http(
                            attempt.credential_id,
                            attempt.attempts,
                            response,
                        )
                        .with_request_meta(request_meta),
                    );
                }

                let code = status.as_u16();
                if matches!(code, 401 | 403 | 429 | 500 | 502 | 503 | 504) {
                    if code == 403 {
                        invalidate_grok_session(&settings, base_url, attempt.material.sso.as_str());
                    }
                    return retry_video_failure(
                        provider,
                        credential_states,
                        &state_manager,
                        attempt.credential_id,
                        model_hint.as_deref(),
                        Some(code),
                        format!("upstream status {code}"),
                        Some(request_meta),
                    );
                }

                let response = match build_openai_error_http_response(response).await {
                    Ok(response) => response,
                    Err(err) => {
                        return retry_video_failure(
                            provider,
                            credential_states,
                            &state_manager,
                            attempt.credential_id,
                            model_hint.as_deref(),
                            Some(code),
                            err.to_string(),
                            Some(request_meta),
                        );
                    }
                };
                state_manager.mark_success(
                    credential_states,
                    &provider.channel,
                    attempt.credential_id,
                );
                CredentialRetryDecision::Return(
                    UpstreamResponse::from_http(attempt.credential_id, attempt.attempts, response)
                        .with_request_meta(request_meta),
                )
            }
        },
    )
    .await
}

impl GrokStoredVideo {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "completed_at": self.completed_at,
            "created_at": self.created_at,
            "model": self.model,
            "object": "video",
            "progress": self.progress,
            "prompt": self.prompt,
            "seconds": self.seconds,
            "size": self.size,
            "status": self.status,
        })
    }
}

fn video_settings(provider: &ProviderDefinition) -> GrokSettings {
    match &provider.settings {
        ChannelSettings::Builtin(BuiltinChannelSettings::Grok(value)) => value.clone(),
        _ => GrokSettings::default(),
    }
}

async fn extract_media_post_id(response: wreq::Response) -> Result<String, String> {
    let body = response.text().await.map_err(|err| err.to_string())?;
    let payload = serde_json::from_str::<Value>(body.as_str()).map_err(|err| err.to_string())?;
    payload
        .pointer("/post/id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| "grok media post create returned no post id".to_string())
}

async fn collect_video_run_result(response: wreq::Response) -> Result<GrokVideoRunResult, String> {
    let mut result = GrokVideoRunResult::default();
    let mut line_buffer = Vec::new();
    let mut upstream = response.bytes_stream();
    while let Some(item) = upstream.next().await {
        let chunk = item.map_err(|err| err.to_string())?;
        line_buffer.extend_from_slice(chunk.as_ref());
        while let Some(line) = next_line(&mut line_buffer) {
            apply_video_result_line(&mut result, line.as_slice())?;
        }
    }
    if !line_buffer.is_empty() {
        apply_video_result_line(&mut result, line_buffer.as_slice())?;
    }
    Ok(result)
}

fn apply_video_result_line(result: &mut GrokVideoRunResult, line: &[u8]) -> Result<(), String> {
    if line.is_empty() {
        return Ok(());
    }
    let value = serde_json::from_slice::<Value>(line).map_err(|err| err.to_string())?;
    let Some(response) = value.pointer("/result/response").and_then(Value::as_object) else {
        return Ok(());
    };

    if let Some(response_id) = response
        .get("responseId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        result.response_id = Some(response_id.to_string());
    }
    if let Some(model_response) = response.get("modelResponse").and_then(Value::as_object) {
        if let Some(response_id) = model_response
            .get("responseId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            result.response_id = Some(response_id.to_string());
        }
    }
    if let Some(video_response) = response
        .get("streamingVideoGenerationResponse")
        .and_then(Value::as_object)
    {
        if let Some(video_url) = video_response
            .get("videoUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            result.video_url = Some(video_url.to_string());
        }
        if let Some(thumbnail_url) = video_response
            .get("thumbnailImageUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            result.thumbnail_url = Some(thumbnail_url.to_string());
        }
    }
    Ok(())
}

async fn retry_or_return_video_http_error(
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    state_manager: &CredentialStateManager,
    settings: &GrokSettings,
    base_url: &str,
    sso: &str,
    credential_id: i64,
    attempts: usize,
    model: Option<&str>,
    response: wreq::Response,
    request_meta: Option<crate::channels::upstream::UpstreamRequestMeta>,
) -> CredentialRetryDecision<UpstreamResponse> {
    let status = response.status();
    let code = status.as_u16();
    if matches!(code, 401 | 403 | 429 | 500 | 502 | 503 | 504) {
        if code == 403 {
            invalidate_grok_session(settings, base_url, sso);
        }
        return retry_video_failure(
            provider,
            credential_states,
            state_manager,
            credential_id,
            model,
            Some(code),
            format!("upstream status {code}"),
            request_meta,
        );
    }

    match build_openai_error_http_response(response).await {
        Ok(response) => {
            state_manager.mark_success(credential_states, &provider.channel, credential_id);
            let upstream = UpstreamResponse::from_http(credential_id, attempts, response);
            CredentialRetryDecision::Return(match request_meta {
                Some(request_meta) => upstream.with_request_meta(request_meta),
                None => upstream,
            })
        }
        Err(err) => retry_video_failure(
            provider,
            credential_states,
            state_manager,
            credential_id,
            model,
            Some(code),
            err.to_string(),
            request_meta,
        ),
    }
}

fn retry_video_failure(
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    state_manager: &CredentialStateManager,
    credential_id: i64,
    model: Option<&str>,
    status: Option<u16>,
    message: String,
    request_meta: Option<crate::channels::upstream::UpstreamRequestMeta>,
) -> CredentialRetryDecision<UpstreamResponse> {
    match status {
        Some(401 | 403) => {
            state_manager.mark_auth_dead(
                credential_states,
                &provider.channel,
                credential_id,
                Some(message.clone()),
            );
        }
        Some(429) => {
            state_manager.mark_rate_limited(
                credential_states,
                &provider.channel,
                credential_id,
                model,
                None,
                Some(message.clone()),
            );
        }
        _ => {
            state_manager.mark_transient_failure(
                credential_states,
                &provider.channel,
                credential_id,
                model,
                None,
                Some(message.clone()),
            );
        }
    }
    CredentialRetryDecision::Retry {
        last_status: status,
        last_error: Some(message),
        last_request_meta: request_meta,
    }
}
