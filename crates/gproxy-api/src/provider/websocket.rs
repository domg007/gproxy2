use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use futures_util::StreamExt;

use gproxy_sdk::provider::engine::{ExecuteBody, ExecuteRequest, UpstreamWebSocket, WsMessage};
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
    let _user_key = authenticate_user(&headers, &state)?;
    let model = params.model.clone();
    let headers_clone = headers.clone();

    Ok(ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_openai_ws(state, provider_name, model, headers_clone, socket).await {
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
    let _user_key = authenticate_user(&headers, &state)?;
    let path = format!("/v1beta/models/{target}");

    Ok(ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_gemini_live_ws(state, provider_name, path, socket).await {
            tracing::warn!(error = %e, "gemini live websocket error");
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
    headers: HeaderMap,
    mut downstream: WebSocket,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Try upstream WebSocket via SDK
    match state
        .engine()
        .connect_upstream_ws(&provider_name, "/v1/responses", model.as_deref())
        .await
    {
        Ok(upstream) => {
            tracing::info!(provider = %provider_name, "websocket bridge active");
            run_ws_bridge(&mut downstream, upstream).await;
        }
        Err(e) => {
            tracing::info!(provider = %provider_name, error = %e, "WS failed, HTTP SSE fallback");
            run_http_sse_fallback(state, provider_name, model, headers, &mut downstream).await?;
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
    path: String,
    mut downstream: WebSocket,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let upstream = state
        .engine()
        .connect_upstream_ws(&provider_name, &path, None)
        .await
        .map_err(|e| format!("gemini live connect failed: {e}"))?;

    tracing::info!(provider = %provider_name, "gemini live websocket bridge active");
    run_ws_bridge(&mut downstream, upstream).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Bidirectional WS bridge
// ---------------------------------------------------------------------------

async fn run_ws_bridge(downstream: &mut WebSocket, mut upstream: UpstreamWebSocket) {
    loop {
        tokio::select! {
            ds_msg = downstream.recv() => {
                match ds_msg {
                    Some(Ok(Message::Text(t))) => {
                        if upstream.send(WsMessage::text(t.to_string())).await.is_err() { break; }
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
                        if downstream.send(Message::Text(t.to_string().into())).await.is_err() { break; }
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
}

// ---------------------------------------------------------------------------
// HTTP SSE fallback
// ---------------------------------------------------------------------------

async fn run_http_sse_fallback(
    state: Arc<AppState>,
    provider_name: String,
    model: Option<String>,
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

        // Execute via SDK engine
        let result = state
            .engine()
            .execute(ExecuteRequest {
                provider: provider_name.clone(),
                operation: "stream_generate_content".to_string(),
                protocol: "openai_response".to_string(),
                body: serde_json::to_vec(&body).unwrap_or_default(),
                headers: headers.clone(),
                model: model.clone(),
            })
            .await;

        match result {
            Ok(result) => {
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
