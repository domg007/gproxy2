use std::sync::Arc;

use axum::body::{Bytes, to_bytes};
use axum::extract::ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{OriginalUri, Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use gproxy_middleware::{
    OperationFamily, ProtocolKind, TransformRequest, TransformRequestPayload, TransformResponse,
    TransformRoute,
};
use gproxy_protocol::claude::model_get::request as claude_model_get_request;
use gproxy_protocol::claude::model_list::request as claude_model_list_request;
use gproxy_protocol::gemini::live::request::GeminiLiveConnectRequest;
use gproxy_protocol::gemini::live::response::GeminiLiveMessageResponse;
use gproxy_protocol::gemini::model_get::request as gemini_model_get_request;
use gproxy_protocol::gemini::model_list::request as gemini_model_list_request;
use gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse;
use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;
use gproxy_protocol::openai::create_response::websocket::request::{
    OpenAiCreateResponseWebSocketConnectRequest,
    QueryParameters as OpenAiCreateResponseWebSocketQueryParameters,
    RequestHeaders as OpenAiCreateResponseWebSocketRequestHeaders,
};
use gproxy_protocol::openai::create_response::websocket::response::OpenAiCreateResponseWebSocketMessageResponse;
use gproxy_protocol::openai::create_response::websocket::types::{
    OpenAiCreateResponseWebSocketClientMessage, OpenAiCreateResponseWebSocketDoneMarker,
    OpenAiCreateResponseWebSocketServerMessage, OpenAiCreateResponseWebSocketWrappedError,
    OpenAiCreateResponseWebSocketWrappedErrorEvent,
    OpenAiCreateResponseWebSocketWrappedErrorEventType,
};
use gproxy_protocol::openai::model_get::request as openai_model_get_request;
use gproxy_protocol::openai::model_list::request as openai_model_list_request;
use gproxy_provider::{
    BuiltinChannel, BuiltinChannelCredential, ChannelCredential, ChannelId, CredentialRef,
    ProviderDefinition, RouteImplementation, RouteKey, UpstreamOAuthRequest, parse_query_value,
};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;
use url::form_urlencoded;

use super::websocket_retry::{UpstreamWebSocket, connect_upstream_websocket_with_credential_retry};
use crate::AppState;

use super::super::{
    HttpError, ModelProtocolPreference, RequestAuthContext, UpstreamResponseMeta,
    anthropic_headers_from_request, apply_credential_update_and_persist, authorize_provider_access,
    bad_request, capture_tracked_http_events, collect_headers, collect_passthrough_headers,
    collect_unscoped_model_ids, collect_websocket_passthrough_headers,
    enqueue_internal_tracked_http_events, enqueue_upstream_request_event_from_meta,
    execute_transform_candidates, execute_transform_request, execute_transform_request_payload,
    internal_error, model_protocol_preference, normalize_gemini_model_path, now_unix_ms,
    oauth_callback_response_to_axum, oauth_response_to_axum, parse_json_body,
    parse_optional_query_value, persist_provider_and_credential, resolve_credential_id,
    resolve_provider, resolve_provider_id, response_from_status_headers_and_bytes,
    split_provider_prefixed_plain_model, upstream_error_request_meta, upstream_error_status,
};

mod http_fallback;
mod request;
mod websocket;

use request::*;
use websocket::*;

pub(in crate::routes::provider) async fn oauth_start(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
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
            let err_request_meta = upstream_error_request_meta(&err);
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                tracked_http_events.as_slice(),
                err_request_meta.as_ref(),
            )
            .await;
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                err_request_meta.as_ref(),
                UpstreamResponseMeta {
                    status: err_status,
                    headers: &[],
                    body: None,
                },
            )
            .await;
            return Err(HttpError::from(err));
        }
    };
    enqueue_upstream_request_event_from_meta(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        None,
        response.request_meta.as_ref(),
        UpstreamResponseMeta {
            status: Some(response.status_code),
            headers: response.headers.as_slice(),
            body: Some(response.body.clone()),
        },
    )
    .await;
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        None,
        tracked_http_events.as_slice(),
        response.request_meta.as_ref(),
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
    let auth = authorize_provider_access(&headers, &state)?;
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
            let err_request_meta = upstream_error_request_meta(&err);
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                tracked_http_events.as_slice(),
                err_request_meta.as_ref(),
            )
            .await;
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                err_request_meta.as_ref(),
                UpstreamResponseMeta {
                    status: err_status,
                    headers: &[],
                    body: None,
                },
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
        auth.downstream_trace_id,
        provider_id,
        resolved_credential_id,
        result.response.request_meta.as_ref(),
        UpstreamResponseMeta {
            status: Some(result.response.status_code),
            headers: result.response.headers.as_slice(),
            body: Some(result.response.body.clone()),
        },
    )
    .await;
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        resolved_credential_id,
        tracked_http_events.as_slice(),
        result.response.request_meta.as_ref(),
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
    let auth = authorize_provider_access(&headers, &state)?;
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
                state.credential_states(),
                credential_id,
                now,
            )
            .await
    })
    .await;
    let upstream = match upstream_result {
        Ok(upstream) => upstream,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                credential_id,
                tracked_http_events.as_slice(),
                err_request_meta.as_ref(),
            )
            .await;
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                credential_id,
                err_request_meta.as_ref(),
                UpstreamResponseMeta {
                    status: err_status,
                    headers: &[],
                    body: None,
                },
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
        auth.downstream_trace_id,
        provider_id,
        upstream_credential_id,
        upstream_request_meta.as_ref(),
        UpstreamResponseMeta {
            status: Some(payload.status_code),
            headers: payload.headers.as_slice(),
            body: Some(payload.body.clone()),
        },
    )
    .await;
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        upstream_credential_id.or(credential_id),
        tracked_http_events.as_slice(),
        upstream_request_meta.as_ref(),
    )
    .await;
    Ok(oauth_response_to_axum(payload))
}

pub(in crate::routes::provider) async fn openai_realtime_upgrade(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, Some(provider_name), uri, headers, ws).await
}

pub(in crate::routes::provider) async fn openai_responses_upgrade(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, Some(provider_name), uri, headers, ws).await
}

pub(in crate::routes::provider) async fn openai_responses_upgrade_unscoped(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, None, uri, headers, ws).await
}

pub(in crate::routes::provider) async fn openai_realtime_upgrade_with_tail(
    State(state): State<Arc<AppState>>,
    Path((provider_name, _tail)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    handle_openai_realtime_upgrade(state, Some(provider_name), uri, headers, ws).await
}

pub(in crate::routes::provider) async fn handle_openai_realtime_upgrade(
    state: Arc<AppState>,
    provider_name: Option<String>,
    uri: Uri,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    Ok(ws.on_upgrade(move |socket| async move {
        let _ =
            run_openai_websocket_session(state, auth, provider_name, uri, headers, socket).await;
    }))
}

pub(in crate::routes::provider) async fn openai_chat_completions(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let value = parse_json_body::<serde_json::Value>(
        &body,
        "invalid openai chat completions request body",
    )?;
    let operation = if stream_enabled(&value) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    };
    let payload = TransformRequestPayload::from_bytes(
        operation,
        ProtocolKind::OpenAiChatCompletion,
        build_openai_payload(
            value,
            &headers,
            "invalid openai chat completions request body",
        )?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_chat_completions_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body = parse_json_body::<serde_json::Value>(
        &body,
        "invalid openai chat completions request body",
    )?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI chat completions request body",
        "`model` in OpenAI chat completions request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI chat completions request body",
    )?;
    let operation = if stream_enabled(&body) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    };
    let body = build_openai_payload(
        body,
        &headers,
        "invalid openai chat completions request body",
    )?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload =
        TransformRequestPayload::from_bytes(operation, ProtocolKind::OpenAiChatCompletion, body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_responses(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let value =
        parse_json_body::<serde_json::Value>(&body, "invalid openai responses request body")?;
    let operation = if stream_enabled(&value) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    };
    let payload = TransformRequestPayload::from_bytes(
        operation,
        ProtocolKind::OpenAi,
        build_openai_payload(value, &headers, "invalid openai responses request body")?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_responses_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body =
        parse_json_body::<serde_json::Value>(&body, "invalid openai responses request body")?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI responses request body",
        "`model` in OpenAI responses request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI responses request body",
    )?;
    let operation = if stream_enabled(&body) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    };
    let body = build_openai_payload(body, &headers, "invalid openai responses request body")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(operation, ProtocolKind::OpenAi, body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_input_tokens(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let value =
        parse_json_body::<serde_json::Value>(&body, "invalid openai input_tokens request body")?;
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(
        OperationFamily::CountToken,
        ProtocolKind::OpenAi,
        build_openai_payload(value, &headers, "invalid openai input_tokens request body")?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_input_tokens_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body =
        parse_json_body::<serde_json::Value>(&body, "invalid openai input_tokens request body")?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI input_tokens request body",
        "`model` in OpenAI input_tokens request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI input_tokens request body",
    )?;
    let body = build_openai_payload(body, &headers, "invalid openai input_tokens request body")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(
        OperationFamily::CountToken,
        ProtocolKind::OpenAi,
        body,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_embeddings(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let value =
        parse_json_body::<serde_json::Value>(&body, "invalid openai embeddings request body")?;
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(
        OperationFamily::Embedding,
        ProtocolKind::OpenAi,
        build_openai_payload(value, &headers, "invalid openai embeddings request body")?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_embeddings_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body =
        parse_json_body::<serde_json::Value>(&body, "invalid openai embeddings request body")?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI embeddings request body",
        "`model` in OpenAI embeddings request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI embeddings request body",
    )?;
    let body = build_openai_payload(body, &headers, "invalid openai embeddings request body")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload =
        TransformRequestPayload::from_bytes(OperationFamily::Embedding, ProtocolKind::OpenAi, body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_compact(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let value = parse_json_body::<serde_json::Value>(&body, "invalid openai compact request body")?;
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(
        OperationFamily::Compact,
        ProtocolKind::OpenAi,
        build_openai_payload(value, &headers, "invalid openai compact request body")?,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn openai_compact_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body =
        parse_json_body::<serde_json::Value>(&body, "invalid openai compact request body")?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in OpenAI compact request body",
        "`model` in OpenAI compact request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in OpenAI compact request body",
    )?;
    let body = build_openai_payload(body, &headers, "invalid openai compact request body")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload =
        TransformRequestPayload::from_bytes(OperationFamily::Compact, ProtocolKind::OpenAi, body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
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
    let passthrough_headers = collect_passthrough_headers(&headers);

    let mut openai = openai_model_list_request::OpenAiModelListRequest::default();
    openai.headers.extra = passthrough_headers.clone();

    let mut claude = claude_model_list_request::ClaudeModelListRequest::default();
    let (version, beta) = anthropic_headers_from_request(&headers);
    claude.headers.anthropic_version = version;
    if beta.is_some() {
        claude.headers.anthropic_beta = beta;
    }
    claude.headers.extra = passthrough_headers.clone();
    claude.query.after_id = parse_query_value(query.as_deref(), "after_id");
    claude.query.before_id = parse_query_value(query.as_deref(), "before_id");
    claude.query.limit = parse_optional_query_value::<u16>(query.as_deref(), "limit")?;

    let mut gemini = gemini_model_list_request::GeminiModelListRequest::default();
    gemini.headers.extra = passthrough_headers;
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
    let passthrough_headers = collect_passthrough_headers(&headers);

    let mut openai = openai_model_get_request::OpenAiModelGetRequest::default();
    openai.path.model = model_id.clone();
    openai.headers.extra = passthrough_headers.clone();

    let mut claude = claude_model_get_request::ClaudeModelGetRequest::default();
    let (version, beta) = anthropic_headers_from_request(&headers);
    claude.headers.anthropic_version = version;
    if beta.is_some() {
        claude.headers.anthropic_beta = beta;
    }
    claude.headers.extra = passthrough_headers.clone();
    claude.path.model_id = model_id.clone();

    let mut gemini = gemini_model_get_request::GeminiModelGetRequest::default();
    gemini.path.name = normalize_gemini_model_path(model_id.as_str())?;
    gemini.headers.extra = passthrough_headers;

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
    let passthrough_headers = collect_passthrough_headers(&headers);

    let mut openai = openai_model_get_request::OpenAiModelGetRequest::default();
    openai.path.model = stripped_model_id.clone();
    openai.headers.extra = passthrough_headers.clone();

    let mut claude = claude_model_get_request::ClaudeModelGetRequest::default();
    let (version, beta) = anthropic_headers_from_request(&headers);
    claude.headers.anthropic_version = version;
    if beta.is_some() {
        claude.headers.anthropic_beta = beta;
    }
    claude.headers.extra = passthrough_headers.clone();
    claude.path.model_id = stripped_model_id.clone();

    let mut gemini = gemini_model_get_request::GeminiModelGetRequest::default();
    gemini.path.name = normalize_gemini_model_path(stripped_model_id.as_str())?;
    gemini.headers.extra = passthrough_headers;

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

#[cfg(test)]
mod tests {
    use super::{
        build_openai_payload, join_base_url_and_path_local, openai_ws_headers_from_upgrade_headers,
        prepare_upstream_websocket_request,
    };
    use axum::http::{HeaderMap, HeaderValue};
    use gproxy_middleware::TransformRequest;
    use gproxy_protocol::gemini::live::request::GeminiLiveConnectRequest;
    use gproxy_protocol::openai::create_response::request::OpenAiCreateResponseRequest;
    use gproxy_provider::{
        BuiltinChannel, BuiltinChannelCredential, BuiltinChannelSettings, ChannelCredential,
        ChannelId, ChannelSettings, CredentialPickMode, CredentialRef, ProviderCredentialState,
        ProviderDefinition, ProviderDispatchTable,
    };
    use serde_json::json;

    fn build_aistudio_provider(base_url: &str, api_key: &str) -> ProviderDefinition {
        let channel = ChannelId::Builtin(BuiltinChannel::AiStudio);
        let mut settings = ChannelSettings::Builtin(BuiltinChannelSettings::default_for(
            BuiltinChannel::AiStudio,
        ));
        if let ChannelSettings::Builtin(BuiltinChannelSettings::AiStudio(value)) = &mut settings {
            value.base_url = base_url.to_string();
        }

        let mut credential = BuiltinChannelCredential::blank_for(BuiltinChannel::AiStudio);
        if let BuiltinChannelCredential::AiStudio(value) = &mut credential {
            value.api_key = api_key.to_string();
        }

        ProviderDefinition {
            channel,
            dispatch: ProviderDispatchTable::default(),
            settings,
            credential_pick_mode: CredentialPickMode::RoundRobinWithCache,
            credentials: ProviderCredentialState {
                credentials: vec![CredentialRef {
                    id: 1,
                    label: None,
                    credential: ChannelCredential::Builtin(credential),
                }],
                channel_states: Vec::new(),
            },
        }
    }

    #[test]
    fn websocket_join_strips_version_suffix_for_live_paths() {
        assert_eq!(
            join_base_url_and_path_local(
                "wss://generativelanguage.googleapis.com/v1beta",
                "/ws/rpc"
            ),
            "wss://generativelanguage.googleapis.com/ws/rpc"
        );
        assert_eq!(
            join_base_url_and_path_local(
                "wss://generativelanguage.googleapis.com/v1beta1",
                "/ws/rpc"
            ),
            "wss://generativelanguage.googleapis.com/ws/rpc"
        );
    }

    #[test]
    fn prepare_aistudio_live_ws_request_injects_key_query() {
        let channel = ChannelId::Builtin(BuiltinChannel::AiStudio);
        let provider = build_aistudio_provider(
            "https://generativelanguage.googleapis.com/v1beta",
            "test-key",
        );
        let request = TransformRequest::GeminiLive(GeminiLiveConnectRequest::default());
        let credential = provider
            .credentials
            .credentials
            .first()
            .expect("provider credential");

        let (url, headers) =
            prepare_upstream_websocket_request(&channel, &provider, &request, credential)
                .expect("prepare websocket request");

        assert!(
            url.starts_with(
                "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?"
            )
        );
        assert!(url.contains("key=test-key"));
        assert!(headers.iter().any(
            |(name, value)| name.eq_ignore_ascii_case("x-goog-api-key") && value == "test-key"
        ));
    }

    #[test]
    fn openai_ws_upgrade_headers_keep_business_headers_and_drop_transport_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer client-secret"),
        );
        headers.insert("user-agent", HeaderValue::from_static("codex_vscode/0.1"));
        headers.insert("connection", HeaderValue::from_static("Upgrade"));
        headers.insert("upgrade", HeaderValue::from_static("websocket"));
        headers.insert("sec-websocket-key", HeaderValue::from_static("abc123"));
        headers.insert(
            "openai-beta",
            HeaderValue::from_static("responses_websockets=2026-02-04"),
        );
        headers.insert(
            "x-codex-turn-metadata",
            HeaderValue::from_static("{\"turn_id\":\"1\"}"),
        );
        headers.insert("session_id", HeaderValue::from_static("sess-123"));
        headers.insert("x-app", HeaderValue::from_static("cli"));

        let parsed = openai_ws_headers_from_upgrade_headers(&headers);

        assert_eq!(
            parsed.openai_beta.as_deref(),
            Some("responses_websockets=2026-02-04")
        );
        assert_eq!(
            parsed.x_codex_turn_metadata.as_deref(),
            Some("{\"turn_id\":\"1\"}")
        );
        assert_eq!(parsed.session_id.as_deref(), Some("sess-123"));
        assert_eq!(parsed.extra.get("x-app").map(String::as_str), Some("cli"));
        assert!(!parsed.extra.contains_key("authorization"));
        assert!(!parsed.extra.contains_key("user-agent"));
        assert!(!parsed.extra.contains_key("connection"));
        assert!(!parsed.extra.contains_key("upgrade"));
        assert!(!parsed.extra.contains_key("sec-websocket-key"));
    }

    #[test]
    fn build_openai_payload_flattens_passthrough_headers_for_typed_decode() {
        let mut headers = HeaderMap::new();
        headers.insert("x-test", HeaderValue::from_static("value"));

        let payload = build_openai_payload(
            json!({
                "model": "claude-sonnet-4-6",
                "input": "hello"
            }),
            &headers,
            "invalid openai responses request body",
        )
        .expect("payload");

        let decoded: OpenAiCreateResponseRequest =
            serde_json::from_slice(payload.as_ref()).expect("request should decode");

        assert_eq!(
            decoded.headers.extra.get("x-test").map(String::as_str),
            Some("value")
        );
    }
}
