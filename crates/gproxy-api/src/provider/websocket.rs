use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use futures_util::StreamExt;

use gproxy_sdk::provider::engine::{
    ExecuteBody, ExecuteRequest, UpstreamWebSocket, WsConnectionResult, WsMessage,
};
use gproxy_server::AppState;

use crate::auth::authenticate_user;
use crate::error::HttpError;

#[derive(serde::Deserialize, Default)]
pub struct WsQueryParams {
    #[serde(default)]
    pub model: Option<String>,
}

/// OpenAI Responses WebSocket: `GET /{provider}/v1/responses`
pub async fn openai_responses_ws(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    Query(params): Query<WsQueryParams>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    let user_key = authenticate_user(&headers, &state)?;
    let model = params.model.clone();

    // Permission check
    if let Some(ref m) = model
        && !state.check_model_permission(user_key.user_id, 0, m) {
            return Err(HttpError::forbidden("model not authorized for this user"));
        }

    // Rate limit check
    if let Some(ref m) = model
        && let Err(rejection) = state.check_rate_limit(user_key.user_id, m) {
            return Err(HttpError::too_many_requests(format!("{rejection:?}")));
        }

    let user_id = user_key.user_id;
    let user_key_id = user_key.id;
    let headers_clone = headers.clone();

    Ok(ws.on_upgrade(move |socket| async move {
        // Record request for rate limit counters
        if let Some(ref m) = model {
            state.record_request(user_id, m);
        }
        if let Err(e) = handle_openai_ws(
            state,
            provider_name,
            model,
            user_id,
            user_key_id,
            headers_clone,
            socket,
        )
        .await
        {
            tracing::warn!(error = %e, "openai responses websocket error");
        }
    }))
}

/// Gemini Live WebSocket: `GET /{provider}/v1beta/models/{target}`
pub async fn gemini_live(
    State(state): State<Arc<AppState>>,
    Path((provider_name, target)): Path<(String, String)>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    let user_key = authenticate_user(&headers, &state)?;

    // Extract model from path (e.g. "gemini-2.0-flash:streamGenerateContent")
    let model = target.split(':').next().map(String::from);

    // Permission check
    if let Some(ref m) = model
        && !state.check_model_permission(user_key.user_id, 0, m) {
            return Err(HttpError::forbidden("model not authorized for this user"));
        }

    // Rate limit check
    if let Some(ref m) = model
        && let Err(rejection) = state.check_rate_limit(user_key.user_id, m) {
            return Err(HttpError::too_many_requests(format!("{rejection:?}")));
        }

    let user_id = user_key.user_id;
    let user_key_id = user_key.id;
    let path = format!("/v1beta/models/{target}");

    Ok(ws.on_upgrade(move |socket| async move {
        if let Some(ref m) = model {
            state.record_request(user_id, m);
        }
        if let Err(e) =
            handle_gemini_live_ws(state, provider_name, model, user_id, user_key_id, path, socket)
                .await
        {
            tracing::warn!(error = %e, "gemini live websocket error");
        }
    }))
}

/// OpenAI Responses WebSocket (unscoped): `GET /v1/responses`
pub async fn openai_responses_ws_unscoped(
    State(state): State<Arc<AppState>>,
    Query(params): Query<WsQueryParams>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, HttpError> {
    let user_key = authenticate_user(&headers, &state)?;
    let model = params.model.clone();

    let Some(model_name) = &model else {
        return Err(HttpError::bad_request(
            "missing model query parameter for unscoped websocket",
        ));
    };

    // Resolve provider from model (alias or provider/model format)
    let (target_provider, target_model) =
        if let Some(alias) = state.resolve_model_alias(model_name) {
            (alias.provider_name, Some(alias.model_id))
        } else if let Some((provider, model)) = model_name.split_once('/') {
            (provider.to_string(), Some(model.to_string()))
        } else {
            return Err(HttpError::bad_request(
                "model must have provider prefix (provider/model) or match an alias",
            ));
        };

    // Permission check
    if let Some(ref m) = target_model
        && !state.check_model_permission(user_key.user_id, 0, m) {
            return Err(HttpError::forbidden("model not authorized for this user"));
        }

    // Rate limit check
    if let Some(ref m) = target_model
        && let Err(rejection) = state.check_rate_limit(user_key.user_id, m) {
            return Err(HttpError::too_many_requests(format!("{rejection:?}")));
        }

    let user_id = user_key.user_id;
    let user_key_id = user_key.id;
    let headers_clone = headers.clone();

    Ok(ws.on_upgrade(move |socket| async move {
        if let Some(ref m) = target_model {
            state.record_request(user_id, m);
        }
        if let Err(e) = handle_openai_ws(
            state,
            target_provider,
            target_model,
            user_id,
            user_key_id,
            headers_clone,
            socket,
        )
        .await
        {
            tracing::warn!(error = %e, "openai responses websocket error (unscoped)");
        }
    }))
}

// ---------------------------------------------------------------------------
// OpenAI: try WS → fallback to HTTP SSE
// ---------------------------------------------------------------------------

async fn handle_openai_ws(
    state: Arc<AppState>,
    provider_name: String,
    model: Option<String>,
    user_id: i64,
    user_key_id: i64,
    headers: HeaderMap,
    mut downstream: WebSocket,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Try upstream WebSocket via SDK
    let ctx = WsBridgeContext {
        state: &state,
        user_id,
        user_key_id,
        model: model.as_deref(),
        operation: "openai_response_websocket",
        protocol: "openai",
    };

    match state
        .engine()
        .connect_upstream_ws(
            &provider_name,
            "openai_response_websocket",
            "openai",
            "/v1/responses",
            model.as_deref(),
        )
        .await
    {
        Ok(WsConnectionResult::Connected(mut upstream)) => {
            tracing::info!(provider = %provider_name, "websocket bridge active (passthrough)");
            let mut bridge = super::ws_bridge::PassthroughBridge::new("openai");
            run_ws_bridge_with_protocol(&mut downstream, &mut upstream, &mut bridge, &ctx).await;
        }
        Ok(WsConnectionResult::NeedsProtocolBridge {
            mut upstream,
            dst_protocol,
            ..
        }) => {
            tracing::info!(provider = %provider_name, dst = %dst_protocol, "websocket bridge active (cross-protocol)");
            let mut bridge: Box<dyn super::ws_bridge::WsProtocolBridge> = match dst_protocol.as_str()
            {
                "gemini" => Box::new(super::ws_bridge::OpenAiToGeminiBridge::new(
                    model.clone(),
                )),
                _ => {
                    tracing::warn!(dst = %dst_protocol, "unsupported cross-protocol WS bridge");
                    return Ok(());
                }
            };
            run_ws_bridge_with_protocol(&mut downstream, &mut upstream, bridge.as_mut(), &ctx)
                .await;
        }
        Err(e) => {
            tracing::info!(provider = %provider_name, error = %e, "WS failed, HTTP SSE fallback");
            run_http_sse_fallback(
                state,
                provider_name,
                model,
                user_id,
                user_key_id,
                headers,
                &mut downstream,
            )
            .await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Gemini Live: WS only (no HTTP fallback)
// ---------------------------------------------------------------------------

async fn handle_gemini_live_ws(
    state: Arc<AppState>,
    provider_name: String,
    model: Option<String>,
    user_id: i64,
    user_key_id: i64,
    path: String,
    mut downstream: WebSocket,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ctx = WsBridgeContext {
        state: &state,
        user_id,
        user_key_id,
        model: model.as_deref(),
        operation: "gemini_live",
        protocol: "gemini",
    };

    let result = state
        .engine()
        .connect_upstream_ws(
            &provider_name,
            "gemini_live",
            "gemini",
            &path,
            model.as_deref(),
        )
        .await
        .map_err(|e| format!("gemini live connect failed: {e}"))?;

    match result {
        WsConnectionResult::Connected(mut upstream) => {
            tracing::info!(provider = %provider_name, "gemini live websocket bridge active (passthrough)");
            let mut bridge = super::ws_bridge::PassthroughBridge::new("gemini");
            run_ws_bridge_with_protocol(&mut downstream, &mut upstream, &mut bridge, &ctx).await;
        }
        WsConnectionResult::NeedsProtocolBridge {
            mut upstream,
            dst_protocol,
            ..
        } => {
            tracing::info!(provider = %provider_name, dst = %dst_protocol, "gemini live websocket bridge active (cross-protocol)");
            let mut bridge: Box<dyn super::ws_bridge::WsProtocolBridge> = match dst_protocol.as_str()
            {
                "openai" => Box::new(super::ws_bridge::GeminiToOpenAiBridge::new(
                    model.clone(),
                )),
                _ => {
                    tracing::warn!(dst = %dst_protocol, "unsupported cross-protocol WS bridge");
                    return Ok(());
                }
            };
            run_ws_bridge_with_protocol(&mut downstream, &mut upstream, bridge.as_mut(), &ctx)
                .await;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Bidirectional WS bridge with protocol conversion and usage tracking
// ---------------------------------------------------------------------------

struct WsBridgeContext<'a> {
    state: &'a AppState,
    user_id: i64,
    user_key_id: i64,
    model: Option<&'a str>,
    operation: &'a str,
    protocol: &'a str,
}

#[allow(clippy::too_many_arguments)]
async fn run_ws_bridge_with_protocol(
    downstream: &mut WebSocket,
    upstream: &mut UpstreamWebSocket,
    bridge: &mut dyn super::ws_bridge::WsProtocolBridge,
    ctx: &WsBridgeContext<'_>,
) {
    loop {
        tokio::select! {
            ds_msg = downstream.recv() => {
                match ds_msg {
                    Some(Ok(Message::Text(t))) => {
                        match bridge.convert_client_message(&t) {
                            Ok(msgs) => {
                                for msg in msgs {
                                    if upstream.send(WsMessage::text(msg)).await.is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "ws bridge: client message conversion failed");
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Binary(b))) => {
                        if upstream.send(WsMessage::binary(b.to_vec())).await.is_err() { break; }
                    }
                    Some(Ok(Message::Ping(p))) => {
                        if upstream.send(WsMessage::ping(p.to_vec())).await.is_err() { break; }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => continue,
                }
            }
            us_msg = upstream.recv() => {
                match us_msg {
                    Some(Ok(WsMessage::Text(t))) => {
                        let text = t.to_string();
                        match bridge.convert_server_message(&text) {
                            Ok((msgs, _usage)) => {
                                for msg in msgs {
                                    if downstream.send(Message::Text(msg.into())).await.is_err() {
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "ws bridge: server message conversion failed");
                                break;
                            }
                        }
                    }
                    Some(Ok(WsMessage::Binary(b))) => {
                        if downstream.send(Message::Binary(b)).await.is_err() { break; }
                    }
                    Some(Ok(WsMessage::Ping(p))) => {
                        if downstream.send(Message::Ping(p)).await.is_err() { break; }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    _ => continue,
                }
            }
        }
    }

    // Record accumulated usage from the WS session
    if let Some(usage) = bridge.final_usage() {
        super::handler::record_usage(
            ctx.state,
            ctx.user_id,
            ctx.user_key_id,
            ctx.model,
            ctx.operation,
            ctx.protocol,
            &usage,
        )
        .await;
    }
}

// ---------------------------------------------------------------------------
// HTTP SSE fallback
// ---------------------------------------------------------------------------

async fn run_http_sse_fallback(
    state: Arc<AppState>,
    provider_name: String,
    model: Option<String>,
    user_id: i64,
    user_key_id: i64,
    headers: HeaderMap,
    downstream: &mut WebSocket,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
        // Read a client WS message
        let text = match downstream.recv().await {
            Some(Ok(Message::Text(t))) => t.to_string(),
            Some(Ok(Message::Binary(b))) => String::from_utf8_lossy(&b).into_owned(),
            Some(Ok(Message::Close(_))) | None => break,
            Some(Ok(Message::Ping(p))) => {
                let _ = downstream.send(Message::Pong(p)).await;
                continue;
            }
            _ => continue,
        };

        // Parse client message, extract body for HTTP
        let client_msg: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                send_ws_error(downstream, &format!("invalid JSON: {e}")).await;
                continue;
            }
        };

        let mut body = client_msg
            .get("response")
            .cloned()
            .unwrap_or(client_msg.clone());
        if let Some(m) = &model {
            body.as_object_mut()
                .map(|o| o.insert("model".to_string(), serde_json::json!(m)));
        }
        body.as_object_mut()
            .map(|o| o.insert("stream".to_string(), serde_json::json!(true)));

        let operation = "stream_generate_content".to_string();
        let protocol = "openai_response".to_string();

        // Execute via SDK engine
        let result = state
            .engine()
            .execute(ExecuteRequest {
                provider: provider_name.clone(),
                operation: operation.clone(),
                protocol: protocol.clone(),
                body: serde_json::to_vec(&body).unwrap_or_default(),
                headers: headers.clone(),
                model: model.clone(),
            })
            .await;

        match result {
            Ok(result) => {
                // Record usage from this request
                if let Some(ref usage) = result.usage {
                    super::handler::record_usage(
                        &state,
                        user_id,
                        user_key_id,
                        model.as_deref(),
                        &operation,
                        &protocol,
                        usage,
                    )
                    .await;
                }

                let mut decoder = gproxy_sdk::protocol::stream::SseToNdjsonRewriter::default();

                match result.body {
                    ExecuteBody::Full(body) => {
                        let mut chunks = Vec::new();
                        chunks.extend(split_sse_events(&decoder.push_chunk(&body)));
                        chunks.extend(split_sse_events(&decoder.finish()));
                        for chunk in chunks {
                            if downstream
                                .send(Message::Text(
                                    String::from_utf8_lossy(&chunk).into_owned().into(),
                                ))
                                .await
                                .is_err()
                            {
                                return Ok(());
                            }
                        }
                    }
                    ExecuteBody::Stream(mut stream) => {
                        while let Some(chunk) = stream.next().await {
                            let chunk = match chunk {
                                Ok(chunk) => chunk,
                                Err(e) => {
                                    send_ws_error(downstream, &e.to_string()).await;
                                    return Ok(());
                                }
                            };
                            for event in split_sse_events(&decoder.push_chunk(&chunk)) {
                                if downstream
                                    .send(Message::Text(
                                        String::from_utf8_lossy(&event).into_owned().into(),
                                    ))
                                    .await
                                    .is_err()
                                {
                                    return Ok(());
                                }
                            }
                        }

                        for event in split_sse_events(&decoder.finish()) {
                            if downstream
                                .send(Message::Text(
                                    String::from_utf8_lossy(&event).into_owned().into(),
                                ))
                                .await
                                .is_err()
                            {
                                return Ok(());
                            }
                        }
                    }
                }
            }
            Err(e) => {
                send_ws_error(downstream, &e.to_string()).await;
            }
        }
    }
    Ok(())
}

async fn send_ws_error(socket: &mut WebSocket, message: &str) {
    let error = serde_json::json!({
        "type": "error",
        "error": {
            "type": "server_error",
            "code": "websocket_proxy_error",
            "message": message,
        }
    });
    let _ = socket.send(Message::Text(error.to_string().into())).await;
}

use gproxy_sdk::protocol::stream::split_lines_owned as split_sse_events;
