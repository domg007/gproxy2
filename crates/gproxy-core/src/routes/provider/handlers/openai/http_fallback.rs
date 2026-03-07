use super::websocket::{
    read_next_openai_client_message, send_openai_server_message, send_wrapped_error_to_socket,
};
use super::*;

pub(super) async fn run_http_fallback_session(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    mut downstream: WebSocket,
    first_message: OpenAiCreateResponseWebSocketClientMessage,
    connect_context: (
        OpenAiCreateResponseWebSocketQueryParameters,
        OpenAiCreateResponseWebSocketRequestHeaders,
    ),
) -> Result<(), String> {
    let (query, headers) = connect_context;
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
