use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use gproxy_middleware::TransformRequest;
use gproxy_protocol::claude::model_get::request as claude_model_get_request;
use gproxy_protocol::claude::model_list::request as claude_model_list_request;
use gproxy_protocol::gemini::model_get::request as gemini_model_get_request;
use gproxy_protocol::gemini::model_list::request as gemini_model_list_request;
use gproxy_protocol::openai::compact_response::request as openai_compact_request;
use gproxy_protocol::openai::count_tokens::request as openai_count_tokens_request;
use gproxy_protocol::openai::create_chat_completions::request as openai_chat_completions_request;
use gproxy_protocol::openai::create_response::request as openai_create_response_request;
use gproxy_protocol::openai::embeddings::request as openai_embeddings_request;
use gproxy_protocol::openai::model_get::request as openai_model_get_request;
use gproxy_protocol::openai::model_list::request as openai_model_list_request;
use gproxy_provider::{
    BuiltinChannel, ChannelId, CredentialRef, UpstreamOAuthRequest, parse_query_value,
};
use serde_json::json;

use crate::AppState;

use super::super::{
    HttpError, ModelProtocolPreference, anthropic_headers_from_request,
    apply_credential_update_and_persist, authorize_provider_access, bad_request,
    capture_tracked_http_events, collect_headers, collect_unscoped_model_ids,
    deserialize_json_scalar, enqueue_internal_tracked_http_events,
    enqueue_upstream_request_event_from_meta, execute_transform_candidates,
    execute_transform_request, internal_error, model_protocol_preference,
    normalize_gemini_model_path, now_unix_ms, oauth_callback_response_to_axum,
    oauth_response_to_axum, parse_optional_query_value, persist_provider_and_credential,
    resolve_credential_id, resolve_provider, resolve_provider_id,
    response_from_status_headers_and_bytes, serialize_json_scalar,
    split_provider_prefixed_plain_model, upstream_error_request_meta, upstream_error_status,
    websocket_upgrade_required_response,
};

pub(in crate::routes::provider) async fn oauth_start(
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
    let (response_result, tracked_http_events) = capture_tracked_http_events(async {
        provider.execute_oauth_start(http.as_ref(), &request).await
    })
    .await;
    let response = match response_result {
        Ok(response) => response,
        Err(err) => {
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                provider_id,
                None,
                tracked_http_events.as_slice(),
            )
            .await;
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
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        provider_id,
        None,
        tracked_http_events.as_slice(),
    )
    .await;
    Ok(oauth_response_to_axum(response))
}

pub(in crate::routes::provider) async fn oauth_callback(
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
    let (callback_result, tracked_http_events) = capture_tracked_http_events(async {
        provider
            .execute_oauth_callback(http.as_ref(), &request)
            .await
    })
    .await;
    let result = match callback_result {
        Ok(result) => result,
        Err(err) => {
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                provider_id,
                None,
                tracked_http_events.as_slice(),
            )
            .await;
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
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        provider_id,
        resolved_credential_id,
        tracked_http_events.as_slice(),
    )
    .await;

    Ok(oauth_callback_response_to_axum(
        result,
        resolved_credential_id,
    ))
}

pub(in crate::routes::provider) async fn upstream_usage(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let provider_id = resolve_provider_id(&state, &channel).await.ok();
    let http = state.load_http();
    let spoof_http = matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode))
        .then(|| state.load_spoof_http());
    let now = now_unix_ms();
    let credential_id = parse_optional_query_value::<i64>(query.as_deref(), "credential_id")?;
    let (upstream_result, tracked_http_events) = capture_tracked_http_events(async {
        provider
            .execute_upstream_usage_with_retry_with_spoof(
                http.as_ref(),
                spoof_http.as_deref(),
                &state.credential_states,
                credential_id,
                now,
            )
            .await
    })
    .await;
    let upstream = match upstream_result {
        Ok(upstream) => upstream,
        Err(err) => {
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                provider_id,
                credential_id,
                tracked_http_events.as_slice(),
            )
            .await;
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
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        provider_id,
        upstream_credential_id.or(credential_id),
        tracked_http_events.as_slice(),
    )
    .await;
    Ok(oauth_response_to_axum(payload))
}

pub(in crate::routes::provider) async fn openai_realtime_upgrade(
    State(state): State<Arc<AppState>>,
    Path(_provider_name): Path<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, headers).await
}

pub(in crate::routes::provider) async fn openai_responses_upgrade(
    State(state): State<Arc<AppState>>,
    Path(_provider_name): Path<String>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, headers).await
}

pub(in crate::routes::provider) async fn openai_responses_upgrade_unscoped(
    State(state): State<Arc<AppState>>,
    _query: RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, headers).await
}

pub(in crate::routes::provider) async fn openai_realtime_upgrade_with_tail(
    State(state): State<Arc<AppState>>,
    Path((_provider_name, _tail)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, headers).await
}

pub(in crate::routes::provider) async fn handle_openai_realtime_upgrade(
    state: Arc<AppState>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    authorize_provider_access(&headers, &state)?;

    Ok(websocket_upgrade_required_response(
        "websocket upstream is not implemented; use /v1/responses (HTTP) for now",
    ))
}

pub(in crate::routes::provider) async fn openai_chat_completions(
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

pub(in crate::routes::provider) async fn openai_chat_completions_unscoped(
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

pub(in crate::routes::provider) async fn openai_responses(
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

pub(in crate::routes::provider) async fn openai_responses_unscoped(
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

pub(in crate::routes::provider) async fn openai_input_tokens(
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

pub(in crate::routes::provider) async fn openai_input_tokens_unscoped(
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

pub(in crate::routes::provider) async fn openai_embeddings(
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

pub(in crate::routes::provider) async fn openai_embeddings_unscoped(
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

pub(in crate::routes::provider) async fn openai_compact(
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

pub(in crate::routes::provider) async fn openai_compact_unscoped(
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

pub(in crate::routes::provider) async fn v1_model_list(
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

    let candidates = match model_protocol_preference(&headers, query.as_deref()) {
        ModelProtocolPreference::Claude => vec![
            TransformRequest::ModelListClaude(claude),
            TransformRequest::ModelListOpenAi(openai),
            TransformRequest::ModelListGemini(gemini),
        ],
        ModelProtocolPreference::Gemini => vec![TransformRequest::ModelListGemini(gemini)],
        ModelProtocolPreference::OpenAi => vec![
            TransformRequest::ModelListOpenAi(openai),
            TransformRequest::ModelListClaude(claude),
            TransformRequest::ModelListGemini(gemini),
        ],
    };

    execute_transform_candidates(state, channel, provider, auth, candidates).await
}

pub(in crate::routes::provider) async fn v1_model_list_unscoped(
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

pub(in crate::routes::provider) async fn v1_model_get(
    State(state): State<Arc<AppState>>,
    Path((provider_name, model_id)): Path<(String, String)>,
    RawQuery(query): RawQuery,
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

    let candidates = match model_protocol_preference(&headers, query.as_deref()) {
        ModelProtocolPreference::Claude => vec![
            TransformRequest::ModelGetClaude(claude),
            TransformRequest::ModelGetOpenAi(openai),
            TransformRequest::ModelGetGemini(gemini),
        ],
        ModelProtocolPreference::Gemini => vec![TransformRequest::ModelGetGemini(gemini)],
        ModelProtocolPreference::OpenAi => vec![
            TransformRequest::ModelGetOpenAi(openai),
            TransformRequest::ModelGetClaude(claude),
            TransformRequest::ModelGetGemini(gemini),
        ],
    };

    execute_transform_candidates(state, channel, provider, auth, candidates).await
}

pub(in crate::routes::provider) async fn v1_model_get_unscoped(
    State(state): State<Arc<AppState>>,
    Path(model_id): Path<String>,
    RawQuery(query): RawQuery,
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

    let candidates = match model_protocol_preference(&headers, query.as_deref()) {
        ModelProtocolPreference::Claude => vec![
            TransformRequest::ModelGetClaude(claude),
            TransformRequest::ModelGetOpenAi(openai),
            TransformRequest::ModelGetGemini(gemini),
        ],
        ModelProtocolPreference::Gemini => vec![TransformRequest::ModelGetGemini(gemini)],
        ModelProtocolPreference::OpenAi => vec![
            TransformRequest::ModelGetOpenAi(openai),
            TransformRequest::ModelGetClaude(claude),
            TransformRequest::ModelGetGemini(gemini),
        ],
    };

    execute_transform_candidates(state, channel, provider, auth, candidates).await
}
