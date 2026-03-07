use super::http_fallback::run_http_fallback_session;
use super::request::{
    openai_ws_headers_from_upgrade_headers, openai_ws_query_from_uri,
    prepare_upstream_websocket_request, route_from_implementation,
    strip_provider_prefix_from_unscoped_openai_ws_message,
    websocket_model_hint_from_upstream_request,
};
use super::*;

pub(super) async fn run_openai_websocket_session(
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
        (query, request_headers),
    )
    .await
}

pub(super) async fn read_next_openai_client_message(
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
                            .send(AxumWsMessage::Binary(payload))
                            .await
                            .map_err(|err| format!("forward upstream binary frame failed: {err}"))?;
                    }
                    TungsteniteMessage::Ping(payload) => {
                        downstream
                            .send(AxumWsMessage::Ping(payload))
                            .await
                            .map_err(|err| format!("forward upstream ping failed: {err}"))?;
                    }
                    TungsteniteMessage::Pong(payload) => {
                        downstream
                            .send(AxumWsMessage::Pong(payload))
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

pub(super) async fn send_wrapped_error_to_socket(
    socket: &mut WebSocket,
    status: Option<u16>,
    message: impl Into<String>,
) -> Result<(), String> {
    let message = build_wrapped_error_message(status, message);
    send_openai_server_message(socket, &message).await
}

pub(super) async fn send_openai_server_message(
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
