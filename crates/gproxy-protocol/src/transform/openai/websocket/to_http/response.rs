use crate::openai::create_response::response::OpenAiCreateResponseResponse;
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

impl TryFrom<&[OpenAiCreateResponseWebSocketMessageResponse]>
    for OpenAiCreateResponseResponse
{
    type Error = crate::transform::utils::TransformError;

    fn try_from(
        value: &[OpenAiCreateResponseWebSocketMessageResponse],
    ) -> Result<Self, crate::transform::utils::TransformError> {
        Ok(websocket_messages_to_openai_nonstream_with_context(value)?.0)
    }
}

impl TryFrom<Vec<OpenAiCreateResponseWebSocketMessageResponse>>
    for OpenAiCreateResponseResponse
{
    type Error = crate::transform::utils::TransformError;

    fn try_from(
        value: Vec<OpenAiCreateResponseWebSocketMessageResponse>,
    ) -> Result<Self, crate::transform::utils::TransformError> {
        OpenAiCreateResponseResponse::try_from(value.as_slice())
    }
}

pub fn websocket_messages_to_openai_nonstream_with_context(
    value: &[OpenAiCreateResponseWebSocketMessageResponse],
) -> Result<
    (
        OpenAiCreateResponseResponse,
        OpenAiWebsocketTransformContext,
    ),
    crate::transform::utils::TransformError,
> {
    let (stream, ctx) = websocket_messages_to_openai_sse_with_context(value)?;
    let response = OpenAiCreateResponseResponse::try_from(stream)?;
    Ok((response, ctx))
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

    #[test]
    fn websocket_messages_map_to_nonstream_response_via_stream_bridge() {
        let message =
            OpenAiCreateResponseWebSocketServerMessage::StreamEvent(ResponseStreamEvent::Completed {
                response: crate::openai::create_response::response::ResponseBody {
                    id: "resp_1".to_string(),
                    created_at: 1,
                    error: None,
                    incomplete_details: None,
                    instructions: None,
                    metadata: Default::default(),
                    model: "gpt-5.3-codex".to_string(),
                    object: crate::openai::create_response::types::ResponseObject::Response,
                    output: vec![],
                    parallel_tool_calls: true,
                    temperature: 1.0,
                    tool_choice: crate::openai::count_tokens::types::ResponseToolChoice::Options(
                        crate::openai::count_tokens::types::ResponseToolChoiceOptions::Auto,
                    ),
                    tools: vec![],
                    top_p: 1.0,
                    background: None,
                    completed_at: None,
                    conversation: None,
                    max_output_tokens: None,
                    max_tool_calls: None,
                    output_text: None,
                    previous_response_id: None,
                    prompt: None,
                    prompt_cache_key: None,
                    prompt_cache_retention: None,
                    reasoning: None,
                    safety_identifier: None,
                    service_tier: None,
                    status: None,
                    text: None,
                    top_logprobs: None,
                    truncation: None,
                    usage: None,
                    user: None,
                },
                sequence_number: 1,
            });

        let response = OpenAiCreateResponseResponse::try_from(vec![message])
            .expect("conversion should succeed");
        match response {
            OpenAiCreateResponseResponse::Success { body, .. } => {
                assert_eq!(body.id, "resp_1");
            }
            _ => panic!("expected non-stream success response"),
        }
    }
}
