use crate::openai::create_response::response::OpenAiCreateResponseResponse;
use crate::openai::create_response::stream::{
    OpenAiCreateResponseSseData, OpenAiCreateResponseSseStreamBody,
};
use crate::openai::create_response::websocket::response::OpenAiCreateResponseWebSocketMessageResponse;
use crate::openai::create_response::websocket::types::{
    OpenAiCreateResponseWebSocketDoneMarker, OpenAiCreateResponseWebSocketServerMessage,
};
use crate::transform::openai::websocket::context::OpenAiWebsocketTransformContext;
use crate::transform::utils::TransformError;

const DONE_MARKER: &str = "[DONE]";

impl TryFrom<&OpenAiCreateResponseSseStreamBody>
    for Vec<OpenAiCreateResponseWebSocketMessageResponse>
{
    type Error = TransformError;

    fn try_from(value: &OpenAiCreateResponseSseStreamBody) -> Result<Self, TransformError> {
        Ok(openai_sse_to_websocket_messages_with_context(value)?.0)
    }
}

impl TryFrom<OpenAiCreateResponseSseStreamBody>
    for Vec<OpenAiCreateResponseWebSocketMessageResponse>
{
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseSseStreamBody) -> Result<Self, TransformError> {
        Vec::<OpenAiCreateResponseWebSocketMessageResponse>::try_from(&value)
    }
}

impl TryFrom<OpenAiCreateResponseResponse> for Vec<OpenAiCreateResponseWebSocketMessageResponse> {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseResponse) -> Result<Self, TransformError> {
        Ok(openai_nonstream_response_to_websocket_messages_with_context(value)?.0)
    }
}

pub fn openai_nonstream_response_to_websocket_messages_with_context(
    value: OpenAiCreateResponseResponse,
) -> Result<
    (
        Vec<OpenAiCreateResponseWebSocketMessageResponse>,
        OpenAiWebsocketTransformContext,
    ),
    TransformError,
> {
    let stream = OpenAiCreateResponseSseStreamBody::try_from(value)?;
    openai_sse_to_websocket_messages_with_context(&stream)
}

pub fn openai_sse_to_websocket_messages_with_context(
    value: &OpenAiCreateResponseSseStreamBody,
) -> Result<
    (
        Vec<OpenAiCreateResponseWebSocketMessageResponse>,
        OpenAiWebsocketTransformContext,
    ),
    TransformError,
> {
    let mut ctx = OpenAiWebsocketTransformContext::default();
    let mut messages = Vec::with_capacity(value.events.len());
    for event in &value.events {
        match &event.data {
            OpenAiCreateResponseSseData::Event(data) => {
                messages.push(OpenAiCreateResponseWebSocketServerMessage::StreamEvent(
                    data.clone(),
                ));
            }
            OpenAiCreateResponseSseData::Done(marker) => {
                if marker != DONE_MARKER {
                    ctx.push_warning(format!(
                        "openai websocket from_http response: unsupported done marker `{marker}` downgraded to [DONE]"
                    ));
                }
                messages.push(OpenAiCreateResponseWebSocketServerMessage::Done(
                    OpenAiCreateResponseWebSocketDoneMarker::Done,
                ));
            }
        }
    }

    Ok((messages, ctx))
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use crate::openai::create_response::response::OpenAiCreateResponseResponse;
    use crate::openai::create_response::response::ResponseBody;
    use crate::openai::create_response::stream::{
        OpenAiCreateResponseSseData, OpenAiCreateResponseSseEvent,
        OpenAiCreateResponseSseStreamBody, ResponseStreamEvent,
    };
    use crate::openai::types::OpenAiResponseHeaders;

    use super::*;

    #[test]
    fn sse_stream_maps_to_websocket_messages() {
        let sse = OpenAiCreateResponseSseStreamBody {
            events: vec![
                OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Event(ResponseStreamEvent::Completed {
                        response: ResponseBody {
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
                    }),
                },
                OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Done("[DONE]".to_string()),
                },
            ],
        };

        let ws = Vec::<OpenAiCreateResponseWebSocketMessageResponse>::try_from(sse)
            .expect("conversion should succeed");
        assert_eq!(ws.len(), 2);
        assert!(matches!(
            ws[1],
            OpenAiCreateResponseWebSocketServerMessage::Done(_)
        ));
    }

    #[test]
    fn unsupported_done_marker_is_downgraded_with_warning() {
        let sse = OpenAiCreateResponseSseStreamBody {
            events: vec![OpenAiCreateResponseSseEvent {
                event: None,
                data: OpenAiCreateResponseSseData::Done("__END__".to_string()),
            }],
        };

        let (ws, ctx) =
            openai_sse_to_websocket_messages_with_context(&sse).expect("conversion should succeed");
        assert_eq!(ws.len(), 1);
        assert!(matches!(
            ws[0],
            OpenAiCreateResponseWebSocketServerMessage::Done(_)
        ));
        assert_eq!(ctx.warnings.len(), 1);
    }

    #[test]
    fn nonstream_response_maps_to_websocket_messages_via_stream_bridge() {
        let response = OpenAiCreateResponseResponse::Success {
            stats_code: StatusCode::OK,
            headers: OpenAiResponseHeaders::default(),
            body: ResponseBody {
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
        };

        let ws = Vec::<OpenAiCreateResponseWebSocketMessageResponse>::try_from(response)
            .expect("conversion should succeed");
        assert_eq!(ws.len(), 4);
        assert!(matches!(
            ws.last().expect("done frame"),
            OpenAiCreateResponseWebSocketServerMessage::Done(_)
        ));
    }
}
