use crate::openai::create_response::stream::{
    OpenAiCreateResponseSseData, OpenAiCreateResponseSseEvent, OpenAiCreateResponseSseStreamBody,
    ResponseStreamEvent,
};
use crate::openai::create_response::websocket::response::OpenAiCreateResponseWebSocketMessageResponse;
use crate::openai::create_response::websocket::types::{
    OpenAiCreateResponseWebSocketServerMessage, OpenAiCreateResponseWebSocketWrappedErrorEvent,
};
use crate::transform::openai::websocket::context::OpenAiWebsocketTransformContext;

const DONE_MARKER: &str = "[DONE]";
const FALLBACK_WS_ERROR_CODE: &str = "websocket_error";
const FALLBACK_WS_ERROR_MESSAGE: &str = "websocket error";

fn wrapped_error_to_stream_event(
    event: OpenAiCreateResponseWebSocketWrappedErrorEvent,
    sequence_number: u64,
    ctx: &mut OpenAiWebsocketTransformContext,
) -> ResponseStreamEvent {
    if let Some(status) = event.status {
        ctx.push_warning(format!(
            "openai websocket to_http response: dropped wrapped error status={status}"
        ));
    }
    if let Some(headers) = event.headers.as_ref() {
        ctx.push_warning(format!(
            "openai websocket to_http response: dropped wrapped error headers ({} entries)",
            headers.len()
        ));
    }
    let payload = event.error.unwrap_or_default();
    ResponseStreamEvent::Error {
        code: payload
            .code
            .or(payload.type_)
            .unwrap_or_else(|| FALLBACK_WS_ERROR_CODE.to_string()),
        message: payload
            .message
            .unwrap_or_else(|| FALLBACK_WS_ERROR_MESSAGE.to_string()),
        param: payload.param,
        sequence_number,
    }
}

fn api_error_to_stream_event(
    event: crate::openai::types::OpenAiApiErrorResponse,
    sequence_number: u64,
) -> ResponseStreamEvent {
    ResponseStreamEvent::Error {
        code: event
            .error
            .code
            .clone()
            .unwrap_or_else(|| event.error.type_.clone()),
        message: event.error.message,
        param: event.error.param,
        sequence_number,
    }
}

impl TryFrom<&[OpenAiCreateResponseWebSocketMessageResponse]>
    for OpenAiCreateResponseSseStreamBody
{
    type Error = crate::transform::utils::TransformError;

    fn try_from(
        value: &[OpenAiCreateResponseWebSocketMessageResponse],
    ) -> Result<Self, Self::Error> {
        Ok(websocket_messages_to_openai_sse_with_context(value)?.0)
    }
}

impl TryFrom<Vec<OpenAiCreateResponseWebSocketMessageResponse>>
    for OpenAiCreateResponseSseStreamBody
{
    type Error = crate::transform::utils::TransformError;

    fn try_from(
        value: Vec<OpenAiCreateResponseWebSocketMessageResponse>,
    ) -> Result<Self, Self::Error> {
        OpenAiCreateResponseSseStreamBody::try_from(value.as_slice())
    }
}

pub fn websocket_messages_to_openai_sse_with_context(
    value: &[OpenAiCreateResponseWebSocketMessageResponse],
) -> Result<
    (
        OpenAiCreateResponseSseStreamBody,
        OpenAiWebsocketTransformContext,
    ),
    crate::transform::utils::TransformError,
> {
    let mut ctx = OpenAiWebsocketTransformContext::default();
    let mut events = Vec::new();
    let mut next_sequence_number = 0_u64;

    for message in value.iter().cloned() {
        match message {
            OpenAiCreateResponseWebSocketServerMessage::StreamEvent(event) => {
                events.push(OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Event(event),
                });
            }
            OpenAiCreateResponseWebSocketServerMessage::Done(_) => {
                events.push(OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Done(DONE_MARKER.to_string()),
                });
            }
            OpenAiCreateResponseWebSocketServerMessage::WrappedError(event) => {
                events.push(OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Event(wrapped_error_to_stream_event(
                        event,
                        next_sequence_number,
                        &mut ctx,
                    )),
                });
                next_sequence_number = next_sequence_number.saturating_add(1);
            }
            OpenAiCreateResponseWebSocketServerMessage::ApiError(event) => {
                events.push(OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Event(api_error_to_stream_event(
                        event,
                        next_sequence_number,
                    )),
                });
                next_sequence_number = next_sequence_number.saturating_add(1);
            }
            // No equivalent SSE event in OpenAI response stream schema.
            OpenAiCreateResponseWebSocketServerMessage::RateLimit(_) => {
                ctx.push_warning(
                    "openai websocket to_http response: dropped codex.rate_limits event"
                        .to_string(),
                );
            }
        }
    }

    Ok((OpenAiCreateResponseSseStreamBody { events }, ctx))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::openai::create_response::websocket::types::OpenAiCreateResponseWebSocketServerMessage;

    use super::*;

    #[test]
    fn wrapped_error_maps_to_sse_error_event() {
        let message: OpenAiCreateResponseWebSocketMessageResponse = serde_json::from_value(json!({
            "type": "error",
            "status": 429,
            "error": {
                "type": "usage_limit_reached",
                "code": "rate_limit_exceeded",
                "message": "slow down"
            }
        }))
        .expect("wrapped websocket error should parse");

        let sse = OpenAiCreateResponseSseStreamBody::try_from(vec![message])
            .expect("conversion should succeed");
        assert_eq!(sse.events.len(), 1);

        match &sse.events[0].data {
            OpenAiCreateResponseSseData::Event(ResponseStreamEvent::Error { code, .. }) => {
                assert_eq!(code, "rate_limit_exceeded");
            }
            _ => panic!("expected OpenAI SSE error event"),
        }
    }

    #[test]
    fn websocket_rate_limit_event_is_ignored_for_sse_mapping() {
        let message: OpenAiCreateResponseWebSocketMessageResponse = serde_json::from_value(json!({
            "type": "codex.rate_limits",
            "rate_limits": {
                "primary": {
                    "used_percent": 90.0
                }
            }
        }))
        .expect("rate-limit event should parse");

        let (sse, ctx) = websocket_messages_to_openai_sse_with_context(&[message])
            .expect("conversion should succeed");
        assert!(sse.events.is_empty());
        assert_eq!(ctx.warnings.len(), 1);
    }

    #[test]
    fn stream_event_passes_through() {
        let message =
            OpenAiCreateResponseWebSocketServerMessage::StreamEvent(ResponseStreamEvent::Error {
                code: "invalid_prompt".to_string(),
                message: "bad prompt".to_string(),
                param: None,
                sequence_number: 1,
            });

        let sse = OpenAiCreateResponseSseStreamBody::try_from(vec![message])
            .expect("conversion should succeed");
        assert_eq!(sse.events.len(), 1);
    }

    #[test]
    fn wrapped_error_status_headers_are_recorded_in_context() {
        let message: OpenAiCreateResponseWebSocketMessageResponse = serde_json::from_value(json!({
            "type": "error",
            "status": 429,
            "error": {
                "type": "usage_limit_reached",
                "code": "rate_limit_exceeded",
                "message": "slow down"
            },
            "headers": {
                "x-codex-primary-used-percent": "100"
            }
        }))
        .expect("wrapped websocket error should parse");

        let (_, ctx) = websocket_messages_to_openai_sse_with_context(&[message])
            .expect("conversion should succeed");
        assert_eq!(ctx.warnings.len(), 2);
    }
}
