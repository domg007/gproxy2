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
use gproxy_protocol::openai::compact_response::request as openai_compact_request;
use gproxy_protocol::openai::count_tokens::request as openai_count_tokens_request;
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
use gproxy_protocol::openai::embeddings::request as openai_embeddings_request;
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
    bad_request, capture_tracked_http_events, collect_headers, collect_unscoped_model_ids,
    enqueue_internal_tracked_http_events, enqueue_upstream_request_event_from_meta,
    execute_transform_candidates, execute_transform_request, execute_transform_request_payload,
    internal_error, model_protocol_preference, normalize_gemini_model_path, now_unix_ms,
    oauth_callback_response_to_axum, oauth_response_to_axum, parse_json_body,
    parse_optional_query_value, persist_provider_and_credential, resolve_credential_id,
    resolve_provider, resolve_provider_id, response_from_status_headers_and_bytes,
    split_provider_prefixed_plain_model, upstream_error_request_meta, upstream_error_status,
};

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

async fn run_openai_websocket_session(
    state: Arc<AppState>,
    auth: RequestAuthContext,
    scoped_provider_name: Option<String>,
    uri: Uri,
    headers: HeaderMap,
    mut downstream: WebSocket,
) -> Result<(), String> {
    let Some(mut first_client_message) = read_next_openai_client_message(&mut downstream).await?
    else {
        return Ok(());
    };

    let (channel, provider) = match scoped_provider_name {
        Some(provider_name) => resolve_provider(&state, provider_name.as_str())
            .map_err(|err| format!("resolve provider failed: {err:?}"))?,
        None => {
            let provider_name =
                strip_provider_prefix_from_unscoped_openai_ws_message(&mut first_client_message)?;
            resolve_provider(&state, provider_name.as_str())
                .map_err(|err| format!("resolve provider failed: {err:?}"))?
        }
    };

    let query = openai_ws_query_from_uri(&uri);
    let request_headers = openai_ws_headers_from_upgrade_headers(&headers);
    let downstream_connect = OpenAiCreateResponseWebSocketConnectRequest {
        query: query.clone(),
        headers: request_headers.clone(),
        body: Some(first_client_message.clone()),
        ..OpenAiCreateResponseWebSocketConnectRequest::default()
    };
    let downstream_request = TransformRequest::OpenAiResponseWebSocket(downstream_connect.clone());

    let src_route = RouteKey::new(
        OperationFamily::OpenAiResponseWebSocket,
        ProtocolKind::OpenAi,
    );
    let Some(implementation) = provider.dispatch.resolve(src_route).cloned() else {
        send_wrapped_error_to_socket(
            &mut downstream,
            Some(StatusCode::NOT_IMPLEMENTED.as_u16()),
            "provider does not support OpenAI websocket route",
        )
        .await?;
        return Ok(());
    };
    let Some(route) = route_from_implementation(src_route, implementation) else {
        send_wrapped_error_to_socket(
            &mut downstream,
            Some(StatusCode::NOT_IMPLEMENTED.as_u16()),
            "provider websocket route is unsupported",
        )
        .await?;
        return Ok(());
    };

    if matches!(
        route.dst_operation,
        OperationFamily::OpenAiResponseWebSocket | OperationFamily::GeminiLive
    ) {
        let now = now_unix_ms();
        if let Ok(upstream_request) =
            gproxy_middleware::transform_request(downstream_request.clone(), route)
                .map_err(|err| err.to_string())
        {
            let model_hint = websocket_model_hint_from_upstream_request(&upstream_request);
            if let Ok(mut upstream) = connect_upstream_websocket_with_credential_retry(
                state.as_ref(),
                &channel,
                &provider,
                model_hint.as_deref(),
                now,
                |credential| {
                    prepare_upstream_websocket_request(
                        &channel,
                        &provider,
                        &upstream_request,
                        credential,
                    )
                },
            )
            .await
            {
                return run_direct_websocket_bridge_loop(
                    downstream,
                    &mut upstream,
                    route,
                    upstream_request,
                    query,
                    request_headers,
                )
                .await;
            }
        }
    }

    run_http_fallback_session(
        state,
        channel,
        provider,
        auth,
        downstream,
        first_client_message,
        query,
        request_headers,
    )
    .await
}

fn route_from_implementation(
    src_route: RouteKey,
    implementation: RouteImplementation,
) -> Option<TransformRoute> {
    match implementation {
        RouteImplementation::Passthrough => Some(TransformRoute {
            src_operation: src_route.operation,
            src_protocol: src_route.protocol,
            dst_operation: src_route.operation,
            dst_protocol: src_route.protocol,
        }),
        RouteImplementation::TransformTo { destination } => Some(TransformRoute {
            src_operation: src_route.operation,
            src_protocol: src_route.protocol,
            dst_operation: destination.operation,
            dst_protocol: destination.protocol,
        }),
        RouteImplementation::Local | RouteImplementation::Unsupported => None,
    }
}

async fn read_next_openai_client_message(
    downstream: &mut WebSocket,
) -> Result<Option<OpenAiCreateResponseWebSocketClientMessage>, String> {
    loop {
        let Some(message) = downstream.recv().await else {
            return Ok(None);
        };
        let message = message.map_err(|err| err.to_string())?;
        match message {
            AxumWsMessage::Text(text) => {
                let parsed: OpenAiCreateResponseWebSocketClientMessage =
                    serde_json::from_str(text.as_ref())
                        .map_err(|err| format!("invalid websocket client frame JSON: {err}"))?;
                return Ok(Some(parsed));
            }
            AxumWsMessage::Binary(bytes) => {
                let text = String::from_utf8(bytes.to_vec())
                    .map_err(|err| err.utf8_error().to_string())?;
                let parsed: OpenAiCreateResponseWebSocketClientMessage =
                    serde_json::from_str(text.as_str())
                        .map_err(|err| format!("invalid websocket client frame JSON: {err}"))?;
                return Ok(Some(parsed));
            }
            AxumWsMessage::Ping(payload) => {
                let _ = downstream.send(AxumWsMessage::Pong(payload)).await;
            }
            AxumWsMessage::Pong(_) => {}
            AxumWsMessage::Close(_) => return Ok(None),
        }
    }
}

fn strip_provider_prefix_from_unscoped_openai_ws_message(
    message: &mut OpenAiCreateResponseWebSocketClientMessage,
) -> Result<String, String> {
    let OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(create) = message else {
        return Err("unscoped websocket first frame must be `response.create`".to_string());
    };
    let Some(model) = create.request.model.clone() else {
        return Err("unscoped websocket `response.create` requires `model`".to_string());
    };
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model.as_str())
        .map_err(|err| format!("invalid unscoped websocket model: {err:?}"))?;
    create.request.model = Some(stripped_model);
    Ok(provider_name)
}

fn openai_ws_query_from_uri(uri: &Uri) -> OpenAiCreateResponseWebSocketQueryParameters {
    let mut query = OpenAiCreateResponseWebSocketQueryParameters::default();
    for (key, value) in form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes()) {
        let key = key.into_owned();
        let value = value.into_owned();
        if key.eq_ignore_ascii_case("api-version") {
            query.api_version = Some(value);
        } else {
            query.extra.insert(key, value);
        }
    }
    query
}

fn openai_ws_headers_from_upgrade_headers(
    headers: &HeaderMap,
) -> OpenAiCreateResponseWebSocketRequestHeaders {
    let mut out = OpenAiCreateResponseWebSocketRequestHeaders::default();
    for (name, value) in headers {
        let Ok(value) = value.to_str() else {
            continue;
        };
        let value = value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        let name_str = name.as_str();
        if name_str.eq_ignore_ascii_case("authorization") {
            out.authorization = Some(value);
        } else if name_str.eq_ignore_ascii_case("openai-beta") {
            out.openai_beta = Some(value);
        } else if name_str.eq_ignore_ascii_case("x-codex-turn-state") {
            out.x_codex_turn_state = Some(value);
        } else if name_str.eq_ignore_ascii_case("x-codex-turn-metadata") {
            out.x_codex_turn_metadata = Some(value);
        } else if name_str.eq_ignore_ascii_case("session_id")
            || name_str.eq_ignore_ascii_case("session-id")
        {
            out.session_id = Some(value);
        } else if name_str.eq_ignore_ascii_case("chatgpt-account-id") {
            out.chatgpt_account_id = Some(value);
        }
    }
    out
}

fn prepare_upstream_websocket_request(
    channel: &ChannelId,
    provider: &ProviderDefinition,
    upstream_request: &TransformRequest,
    credential: &CredentialRef,
) -> Result<(String, Vec<(String, String)>), String> {
    let base_url = to_websocket_base_url(provider.settings.base_url())?;
    match upstream_request {
        TransformRequest::OpenAiResponseWebSocket(request) => {
            let path = if matches!(channel, ChannelId::Builtin(BuiltinChannel::Codex)) {
                "/responses"
            } else {
                "/v1/responses"
            };

            let mut query_pairs = Vec::new();
            if let Some(api_version) = request.query.api_version.as_deref() {
                query_pairs.push(("api-version".to_string(), api_version.to_string()));
            }
            for (key, value) in &request.query.extra {
                query_pairs.push((key.clone(), value.clone()));
            }

            let mut headers = Vec::new();
            if let Some(value) = request.headers.authorization.as_deref() {
                add_or_replace_header(&mut headers, "authorization", value.to_string());
            }
            if let Some(value) = request.headers.openai_beta.as_deref() {
                add_or_replace_header(&mut headers, "openai-beta", value.to_string());
            }
            if let Some(value) = request.headers.x_codex_turn_state.as_deref() {
                add_or_replace_header(&mut headers, "x-codex-turn-state", value.to_string());
            }
            if let Some(value) = request.headers.x_codex_turn_metadata.as_deref() {
                add_or_replace_header(&mut headers, "x-codex-turn-metadata", value.to_string());
            }
            if let Some(value) = request.headers.session_id.as_deref() {
                add_or_replace_header(&mut headers, "session_id", value.to_string());
            }
            if let Some(value) = request.headers.chatgpt_account_id.as_deref() {
                add_or_replace_header(&mut headers, "chatgpt-account-id", value.to_string());
            }
            for (key, value) in &request.headers.extra {
                add_or_replace_header(&mut headers, key.as_str(), value.clone());
            }

            match (&channel, &credential.credential) {
                (
                    ChannelId::Builtin(BuiltinChannel::OpenAi),
                    ChannelCredential::Builtin(BuiltinChannelCredential::OpenAi(value)),
                ) => {
                    add_or_replace_header(
                        &mut headers,
                        "authorization",
                        format!("Bearer {}", value.api_key.trim()),
                    );
                    add_or_replace_header(
                        &mut headers,
                        "user-agent",
                        default_websocket_user_agent(provider),
                    );
                }
                (
                    ChannelId::Builtin(BuiltinChannel::Codex),
                    ChannelCredential::Builtin(BuiltinChannelCredential::Codex(value)),
                ) => {
                    add_or_replace_header(
                        &mut headers,
                        "authorization",
                        format!("Bearer {}", value.access_token.trim()),
                    );
                    add_or_replace_header(
                        &mut headers,
                        "chatgpt-account-id",
                        value.account_id.trim().to_string(),
                    );
                    add_or_replace_header(&mut headers, "originator", "codex_vscode".to_string());
                    if !headers.iter().any(|(name, value)| {
                        name.eq_ignore_ascii_case("openai-beta")
                            && value.contains("responses_websockets=")
                    }) {
                        add_or_replace_header(
                            &mut headers,
                            "openai-beta",
                            "responses_websockets=2026-02-04".to_string(),
                        );
                    }
                    add_or_replace_header(
                        &mut headers,
                        "user-agent",
                        provider
                            .settings
                            .user_agent()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .unwrap_or("codex_vscode/0.99.0")
                            .to_string(),
                    );
                }
                _ => {
                    return Err(format!(
                        "provider {} credential type does not support OpenAI websocket upstream",
                        channel.as_str()
                    ));
                }
            }

            let url = build_websocket_url(base_url.as_str(), path, query_pairs.as_slice());
            Ok((url, headers))
        }
        TransformRequest::GeminiLive(request) => {
            let rpc = match request.path.rpc {
                gproxy_protocol::gemini::live::request::GeminiLiveRpcMethod::BidiGenerateContent => {
                    "google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent"
                }
                gproxy_protocol::gemini::live::request::GeminiLiveRpcMethod::BidiGenerateContentConstrained => {
                    "google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContentConstrained"
                }
            };
            let path = format!("/ws/{rpc}");
            let mut query_pairs = Vec::new();
            if let Some(value) = request
                .query
                .key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                query_pairs.push(("key".to_string(), value.to_string()));
            }
            if let Some(value) = request
                .query
                .access_token
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                query_pairs.push(("access_token".to_string(), value.to_string()));
            }
            let mut headers = Vec::new();
            if let Some(value) = request
                .headers
                .authorization
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                add_or_replace_header(&mut headers, "authorization", value.to_string());
            }

            match (&channel, &credential.credential) {
                (
                    ChannelId::Builtin(BuiltinChannel::AiStudio),
                    ChannelCredential::Builtin(BuiltinChannelCredential::AiStudio(value)),
                ) => {
                    add_or_replace_query(&mut query_pairs, "key", value.api_key.trim().to_string());
                    add_or_replace_header(
                        &mut headers,
                        "x-goog-api-key",
                        value.api_key.trim().to_string(),
                    );
                    add_or_replace_header(
                        &mut headers,
                        "user-agent",
                        default_websocket_user_agent(provider),
                    );
                }
                _ => {
                    return Err(format!(
                        "provider {} credential type does not support Gemini Live websocket upstream",
                        channel.as_str()
                    ));
                }
            }

            let url = build_websocket_url(base_url.as_str(), path.as_str(), query_pairs.as_slice());
            Ok((url, headers))
        }
        _ => Err("upstream transform request is not a websocket request".to_string()),
    }
}

fn openai_model_hint_from_connect_request(
    request: &OpenAiCreateResponseWebSocketConnectRequest,
) -> Option<String> {
    match request.body.as_ref() {
        Some(OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(create)) => {
            create.request.model.clone()
        }
        _ => None,
    }
}

fn gemini_live_model_hint_from_connect_request(
    request: &GeminiLiveConnectRequest,
) -> Option<String> {
    let body = request.body.as_ref()?;
    let value = serde_json::to_value(body).ok()?;
    value
        .pointer("/setup/model")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn websocket_model_hint_from_upstream_request(request: &TransformRequest) -> Option<String> {
    match request {
        TransformRequest::OpenAiResponseWebSocket(value) => {
            openai_model_hint_from_connect_request(value)
        }
        TransformRequest::GeminiLive(value) => gemini_live_model_hint_from_connect_request(value),
        _ => None,
    }
}

fn default_websocket_user_agent(provider: &ProviderDefinition) -> String {
    provider
        .settings
        .user_agent()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            format!(
                "gproxy/{}({},{})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        })
}

fn to_websocket_base_url(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("provider base_url is empty".to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return Ok(format!("wss://{rest}"));
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        return Ok(format!("ws://{rest}"));
    }
    if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        return Ok(trimmed.to_string());
    }
    Err(format!(
        "provider base_url has unsupported scheme: {trimmed}"
    ))
}

fn build_websocket_url(base: &str, path: &str, query: &[(String, String)]) -> String {
    let mut url = join_base_url_and_path_local(base, path);
    if !query.is_empty() {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        for (key, value) in query {
            serializer.append_pair(key, value);
        }
        let encoded = serializer.finish();
        if !encoded.is_empty() {
            if url.contains('?') {
                url.push('&');
            } else {
                url.push('?');
            }
            url.push_str(encoded.as_str());
        }
    }
    url
}

fn join_base_url_and_path_local(base_url: &str, path: &str) -> String {
    let normalized_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let base = base_url.trim_end_matches('/');
    if normalized_path.starts_with("/ws/") {
        if let Some(base_without_v1beta1) = base.strip_suffix("/v1beta1") {
            return format!("{base_without_v1beta1}{normalized_path}");
        }
        if let Some(base_without_v1beta) = base.strip_suffix("/v1beta") {
            return format!("{base_without_v1beta}{normalized_path}");
        }
        if let Some(base_without_v1) = base.strip_suffix("/v1") {
            return format!("{base_without_v1}{normalized_path}");
        }
    }
    if let Some(base_without_v1) = base.strip_suffix("/v1")
        && normalized_path.starts_with("/v1/")
    {
        return format!("{base_without_v1}{normalized_path}");
    }
    if let Some(base_without_v1beta) = base.strip_suffix("/v1beta")
        && normalized_path.starts_with("/v1beta/")
    {
        return format!("{base_without_v1beta}{normalized_path}");
    }
    if let Some(base_without_v1beta1) = base.strip_suffix("/v1beta1")
        && normalized_path.starts_with("/v1beta1/")
    {
        return format!("{base_without_v1beta1}{normalized_path}");
    }
    format!("{base}{normalized_path}")
}

fn add_or_replace_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    if let Some(existing) = headers
        .iter_mut()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
    {
        existing.1 = value;
        return;
    }
    headers.push((name.to_string(), value));
}

fn add_or_replace_query(query: &mut Vec<(String, String)>, name: &str, value: String) {
    if let Some(existing) = query
        .iter_mut()
        .find(|(query_name, _)| query_name.eq_ignore_ascii_case(name))
    {
        existing.1 = value;
        return;
    }
    query.push((name.to_string(), value));
}

async fn run_direct_websocket_bridge_loop(
    mut downstream: WebSocket,
    upstream: &mut UpstreamWebSocket,
    route: TransformRoute,
    first_upstream_request: TransformRequest,
    downstream_query: OpenAiCreateResponseWebSocketQueryParameters,
    downstream_headers: OpenAiCreateResponseWebSocketRequestHeaders,
) -> Result<(), String> {
    if let Some(first_frame) = extract_upstream_ws_frame_text(&first_upstream_request)? {
        upstream
            .send(TungsteniteMessage::Text(first_frame.into()))
            .await
            .map_err(|err| format!("send first upstream websocket frame failed: {err}"))?;
    }

    loop {
        tokio::select! {
            downstream_item = downstream.recv() => {
                let Some(downstream_item) = downstream_item else {
                    let _ = upstream.send(TungsteniteMessage::Close(None)).await;
                    break;
                };
                let downstream_item = downstream_item.map_err(|err| err.to_string())?;
                match downstream_item {
                    AxumWsMessage::Text(text) => {
                        let client_message: OpenAiCreateResponseWebSocketClientMessage = serde_json::from_str(text.as_ref())
                            .map_err(|err| format!("invalid downstream websocket frame JSON: {err}"))?;
                        let downstream_request = TransformRequest::OpenAiResponseWebSocket(
                            OpenAiCreateResponseWebSocketConnectRequest {
                                query: downstream_query.clone(),
                                headers: downstream_headers.clone(),
                                body: Some(client_message),
                                ..OpenAiCreateResponseWebSocketConnectRequest::default()
                            }
                        );
                        let upstream_request = gproxy_middleware::transform_request(downstream_request, route)
                            .map_err(|err| format!("downstream->upstream websocket transform failed: {err}"))?;
                        if let Some(frame_text) = extract_upstream_ws_frame_text(&upstream_request)? {
                            upstream
                                .send(TungsteniteMessage::Text(frame_text.into()))
                                .await
                                .map_err(|err| format!("send upstream websocket frame failed: {err}"))?;
                        }
                    }
                    AxumWsMessage::Binary(payload) => {
                        upstream
                            .send(TungsteniteMessage::Binary(payload))
                            .await
                            .map_err(|err| format!("forward downstream binary frame failed: {err}"))?;
                    }
                    AxumWsMessage::Ping(payload) => {
                        upstream
                            .send(TungsteniteMessage::Ping(payload))
                            .await
                            .map_err(|err| format!("forward downstream ping failed: {err}"))?;
                    }
                    AxumWsMessage::Pong(payload) => {
                        upstream
                            .send(TungsteniteMessage::Pong(payload))
                            .await
                            .map_err(|err| format!("forward downstream pong failed: {err}"))?;
                    }
                    AxumWsMessage::Close(frame) => {
                        let _ = upstream.send(TungsteniteMessage::Close(frame.map(|value| {
                            tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(value.code),
                                reason: value.reason.to_string().into(),
                            }
                        }))).await;
                        break;
                    }
                }
            }
            upstream_item = upstream.next() => {
                let Some(upstream_item) = upstream_item else {
                    let _ = downstream.send(AxumWsMessage::Close(None)).await;
                    break;
                };
                let upstream_item = upstream_item.map_err(|err| err.to_string())?;
                match upstream_item {
                    TungsteniteMessage::Text(text) => {
                        let upstream_response = decode_upstream_ws_text_to_transform_response(route.dst_operation, text.as_ref())?;
                        let downstream_response = gproxy_middleware::transform_response(upstream_response, route)
                            .map_err(|err| format!("upstream->downstream websocket transform failed: {err}"))?;
                        let messages = extract_openai_ws_messages(downstream_response)?;
                        for message in messages {
                            send_openai_server_message(&mut downstream, &message).await?;
                        }
                    }
                    TungsteniteMessage::Binary(payload) => {
                        downstream
                            .send(AxumWsMessage::Binary(payload.into()))
                            .await
                            .map_err(|err| format!("forward upstream binary frame failed: {err}"))?;
                    }
                    TungsteniteMessage::Ping(payload) => {
                        downstream
                            .send(AxumWsMessage::Ping(payload.into()))
                            .await
                            .map_err(|err| format!("forward upstream ping failed: {err}"))?;
                    }
                    TungsteniteMessage::Pong(payload) => {
                        downstream
                            .send(AxumWsMessage::Pong(payload.into()))
                            .await
                            .map_err(|err| format!("forward upstream pong failed: {err}"))?;
                    }
                    TungsteniteMessage::Close(frame) => {
                        let _ = downstream.send(AxumWsMessage::Close(frame.map(|value| {
                            axum::extract::ws::CloseFrame {
                                code: value.code.into(),
                                reason: value.reason.to_string().into(),
                            }
                        }))).await;
                        break;
                    }
                    TungsteniteMessage::Frame(_) => {}
                }
            }
        }
    }

    Ok(())
}

fn extract_upstream_ws_frame_text(request: &TransformRequest) -> Result<Option<String>, String> {
    match request {
        TransformRequest::OpenAiResponseWebSocket(value) => value
            .body
            .as_ref()
            .map(|message| serde_json::to_string(message).map_err(|err| err.to_string()))
            .transpose(),
        TransformRequest::GeminiLive(value) => value
            .body
            .as_ref()
            .map(|message| serde_json::to_string(message).map_err(|err| err.to_string()))
            .transpose(),
        _ => Err("upstream transform request is not a websocket request".to_string()),
    }
}

fn decode_upstream_ws_text_to_transform_response(
    operation: OperationFamily,
    text: &str,
) -> Result<TransformResponse, String> {
    match operation {
        OperationFamily::OpenAiResponseWebSocket => {
            Ok(TransformResponse::OpenAiResponseWebSocket(vec![
                parse_openai_ws_server_message(text)?,
            ]))
        }
        OperationFamily::GeminiLive => {
            let message: GeminiLiveMessageResponse =
                serde_json::from_str(text).map_err(|err| err.to_string())?;
            Ok(TransformResponse::GeminiLive(vec![message]))
        }
        _ => Err("upstream websocket operation is not supported".to_string()),
    }
}

fn parse_openai_ws_server_message(
    text: &str,
) -> Result<OpenAiCreateResponseWebSocketMessageResponse, String> {
    if text.trim() == "[DONE]" {
        return Ok(OpenAiCreateResponseWebSocketServerMessage::Done(
            OpenAiCreateResponseWebSocketDoneMarker::Done,
        ));
    }
    serde_json::from_str(text).map_err(|err| err.to_string())
}

fn extract_openai_ws_messages(
    response: TransformResponse,
) -> Result<Vec<OpenAiCreateResponseWebSocketMessageResponse>, String> {
    match response {
        TransformResponse::OpenAiResponseWebSocket(messages) => Ok(messages),
        _ => Err("downstream transform response is not OpenAI websocket".to_string()),
    }
}

async fn run_http_fallback_session(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    mut downstream: WebSocket,
    first_message: OpenAiCreateResponseWebSocketClientMessage,
    query: OpenAiCreateResponseWebSocketQueryParameters,
    headers: OpenAiCreateResponseWebSocketRequestHeaders,
) -> Result<(), String> {
    let mut pending = Some(first_message);
    loop {
        let message = if let Some(message) = pending.take() {
            message
        } else {
            let Some(message) = read_next_openai_client_message(&mut downstream).await? else {
                return Ok(());
            };
            message
        };

        let downstream_connect = OpenAiCreateResponseWebSocketConnectRequest {
            query: query.clone(),
            headers: headers.clone(),
            body: Some(message),
            ..OpenAiCreateResponseWebSocketConnectRequest::default()
        };
        let stream_request = gproxy_middleware::transform_request(
            TransformRequest::OpenAiResponseWebSocket(downstream_connect),
            TransformRoute {
                src_operation: OperationFamily::OpenAiResponseWebSocket,
                src_protocol: ProtocolKind::OpenAi,
                dst_operation: OperationFamily::StreamGenerateContent,
                dst_protocol: ProtocolKind::OpenAi,
            },
        )
        .map_err(|err| err.to_string())?;

        match execute_transform_request(
            state.clone(),
            channel.clone(),
            provider.clone(),
            auth,
            stream_request,
        )
        .await
        {
            Ok(response) => {
                forward_http_response_to_openai_websocket(response, &mut downstream).await?;
            }
            Err(err) => {
                send_wrapped_error_to_socket(
                    &mut downstream,
                    Some(err.http_status_code()),
                    err.to_string(),
                )
                .await?;
            }
        }
    }
}

async fn forward_http_response_to_openai_websocket(
    response: Response,
    downstream: &mut WebSocket,
) -> Result<(), String> {
    let status = response.status();
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let (parts, body) = response.into_parts();
    let body_bytes = to_bytes(body, 50 * 1024 * 1024)
        .await
        .map_err(|err| err.to_string())?;
    if !status.is_success() {
        let message = parse_error_message_from_json(body_bytes.as_ref())
            .unwrap_or_else(|| format!("upstream status {}", status.as_u16()));
        send_wrapped_error_to_socket(downstream, Some(status.as_u16()), message).await?;
        return Ok(());
    }

    if content_type.contains("text/event-stream") {
        send_sse_bytes_as_openai_websocket_events(body_bytes.as_ref(), downstream).await?;
        return Ok(());
    }

    let parsed = serde_json::from_slice::<OpenAiCreateResponseResponse>(body_bytes.as_ref())
        .map_err(|err| format!("invalid OpenAI response body in HTTP fallback: {err}"))?;
    let messages = Vec::<OpenAiCreateResponseWebSocketMessageResponse>::try_from(parsed)
        .map_err(|err| err.to_string())?;
    for message in messages {
        send_openai_server_message(downstream, &message).await?;
    }
    let _ = parts;
    Ok(())
}

async fn send_sse_bytes_as_openai_websocket_events(
    bytes: &[u8],
    downstream: &mut WebSocket,
) -> Result<(), String> {
    let payload = String::from_utf8_lossy(bytes);
    for raw_line in payload.lines() {
        let line = raw_line.trim_end_matches('\r');
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            send_openai_server_message(
                downstream,
                &OpenAiCreateResponseWebSocketServerMessage::Done(
                    OpenAiCreateResponseWebSocketDoneMarker::Done,
                ),
            )
            .await?;
            continue;
        }
        let event: ResponseStreamEvent = serde_json::from_str(data)
            .map_err(|err| format!("invalid SSE event payload in HTTP fallback: {err}"))?;
        send_openai_server_message(
            downstream,
            &OpenAiCreateResponseWebSocketServerMessage::StreamEvent(event),
        )
        .await?;
    }
    Ok(())
}

fn parse_error_message_from_json(bytes: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
    value
        .pointer("/error/message")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .pointer("/message")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn build_wrapped_error_message(
    status: Option<u16>,
    message: impl Into<String>,
) -> OpenAiCreateResponseWebSocketServerMessage {
    OpenAiCreateResponseWebSocketServerMessage::WrappedError(
        OpenAiCreateResponseWebSocketWrappedErrorEvent {
            type_: OpenAiCreateResponseWebSocketWrappedErrorEventType::Error,
            status,
            error: Some(OpenAiCreateResponseWebSocketWrappedError {
                type_: Some("server_error".to_string()),
                code: Some("websocket_proxy_error".to_string()),
                message: Some(message.into()),
                param: None,
                extra: Default::default(),
            }),
            headers: None,
        },
    )
}

async fn send_wrapped_error_to_socket(
    socket: &mut WebSocket,
    status: Option<u16>,
    message: impl Into<String>,
) -> Result<(), String> {
    let message = build_wrapped_error_message(status, message);
    send_openai_server_message(socket, &message).await
}

async fn send_openai_server_message(
    socket: &mut WebSocket,
    message: &OpenAiCreateResponseWebSocketMessageResponse,
) -> Result<(), String> {
    let text = encode_openai_ws_server_message(message)?;
    socket
        .send(AxumWsMessage::Text(text.into()))
        .await
        .map_err(|err| err.to_string())
}

fn encode_openai_ws_server_message(
    message: &OpenAiCreateResponseWebSocketMessageResponse,
) -> Result<String, String> {
    if matches!(
        message,
        OpenAiCreateResponseWebSocketServerMessage::Done(
            OpenAiCreateResponseWebSocketDoneMarker::Done
        )
    ) {
        return Ok("[DONE]".to_string());
    }
    serde_json::to_string(message).map_err(|err| err.to_string())
}

fn required_string_field<'a>(
    value: &'a serde_json::Value,
    pointer: &str,
    missing_message: &str,
    invalid_message: &str,
) -> Result<&'a str, HttpError> {
    let Some(raw) = value.pointer(pointer) else {
        return Err(bad_request(missing_message));
    };
    raw.as_str().ok_or_else(|| bad_request(invalid_message))
}

fn set_string_field(
    value: &mut serde_json::Value,
    pointer: &str,
    new_value: String,
    missing_message: &str,
) -> Result<(), HttpError> {
    let Some(slot) = value.pointer_mut(pointer) else {
        return Err(bad_request(missing_message));
    };
    *slot = serde_json::Value::String(new_value);
    Ok(())
}

fn stream_enabled(value: &serde_json::Value) -> bool {
    value
        .pointer("/stream")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn encode_json_value(value: &serde_json::Value, context: &str) -> Result<Bytes, HttpError> {
    serde_json::to_vec(value)
        .map(Bytes::from)
        .map_err(|err| bad_request(format!("{context}: {err}")))
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
    let payload =
        TransformRequestPayload::from_bytes(operation, ProtocolKind::OpenAiChatCompletion, body);
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
    let body = encode_json_value(&body, "invalid openai chat completions request body")?;
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
    let payload = TransformRequestPayload::from_bytes(operation, ProtocolKind::OpenAi, body);
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
    let body = encode_json_value(&body, "invalid openai responses request body")?;
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
    let _ = parse_json_body::<openai_count_tokens_request::RequestBody>(
        &body,
        "invalid openai input_tokens request body",
    )?;
    let auth = authorize_provider_access(&headers, &state)?;
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
    let body = encode_json_value(&body, "invalid openai input_tokens request body")?;
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
    let _ = parse_json_body::<openai_embeddings_request::RequestBody>(
        &body,
        "invalid openai embeddings request body",
    )?;
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload =
        TransformRequestPayload::from_bytes(OperationFamily::Embedding, ProtocolKind::OpenAi, body);
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
    let body = encode_json_value(&body, "invalid openai embeddings request body")?;
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
    let _ = parse_json_body::<openai_compact_request::RequestBody>(
        &body,
        "invalid openai compact request body",
    )?;
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload =
        TransformRequestPayload::from_bytes(OperationFamily::Compact, ProtocolKind::OpenAi, body);
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
    let body = encode_json_value(&body, "invalid openai compact request body")?;
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

#[cfg(test)]
mod tests {
    use super::{join_base_url_and_path_local, prepare_upstream_websocket_request};
    use gproxy_middleware::TransformRequest;
    use gproxy_protocol::gemini::live::request::GeminiLiveConnectRequest;
    use gproxy_provider::{
        BuiltinChannel, BuiltinChannelCredential, BuiltinChannelSettings, ChannelCredential,
        ChannelId, ChannelSettings, CredentialPickMode, CredentialRef, ProviderCredentialState,
        ProviderDefinition, ProviderDispatchTable,
    };

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
}
