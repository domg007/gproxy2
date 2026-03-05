use crate::gemini::count_tokens::types::{GeminiContent, GeminiPart};
use crate::gemini::generate_content::request::GeminiGenerateContentRequest;
use crate::gemini::live::request::GeminiLiveConnectRequest;
use crate::gemini::live::types::{
    GeminiBidiGenerateContentClientContent, GeminiBidiGenerateContentClientMessage,
    GeminiBidiGenerateContentClientMessageType, GeminiBidiGenerateContentSetup,
    GeminiBidiGenerateContentToolResponse, GeminiFunctionResponse,
};
use crate::gemini::stream_generate_content::request::GeminiStreamGenerateContentRequest;
use crate::transform::gemini::model_get::utils::ensure_models_prefix;
use crate::transform::gemini::websocket::context::GeminiWebsocketTransformContext;
use crate::transform::utils::TransformError;

fn setup_message(
    request: &GeminiStreamGenerateContentRequest,
) -> GeminiBidiGenerateContentClientMessage {
    GeminiBidiGenerateContentClientMessage {
        message_type: GeminiBidiGenerateContentClientMessageType::Setup {
            setup: GeminiBidiGenerateContentSetup {
                model: ensure_models_prefix(&request.path.model),
                generation_config: request.body.generation_config.clone(),
                system_instruction: request.body.system_instruction.clone(),
                tools: request.body.tools.clone(),
                ..GeminiBidiGenerateContentSetup::default()
            },
        },
    }
}

fn content_message(turns: Vec<GeminiContent>) -> Option<GeminiBidiGenerateContentClientMessage> {
    if turns.is_empty() {
        return None;
    }

    Some(GeminiBidiGenerateContentClientMessage {
        message_type: GeminiBidiGenerateContentClientMessageType::ClientContent {
            client_content: GeminiBidiGenerateContentClientContent {
                turns: Some(turns),
                turn_complete: Some(true),
            },
        },
    })
}

fn part_as_pure_function_response(part: &GeminiPart) -> Option<GeminiFunctionResponse> {
    let function_response = part.function_response.clone()?;
    let has_non_response_fields = part.text.is_some()
        || part.inline_data.is_some()
        || part.function_call.is_some()
        || part.file_data.is_some()
        || part.executable_code.is_some()
        || part.code_execution_result.is_some();
    if has_non_response_fields {
        return None;
    }
    Some(function_response)
}

fn split_turns_and_tool_responses(
    request: &GeminiStreamGenerateContentRequest,
    _ctx: &mut GeminiWebsocketTransformContext,
) -> (Vec<GeminiContent>, Vec<GeminiFunctionResponse>) {
    let mut turns = Vec::new();
    let mut function_responses = Vec::new();

    for content in &request.body.contents {
        let extracted = content
            .parts
            .iter()
            .map(part_as_pure_function_response)
            .collect::<Option<Vec<_>>>();
        if let Some(responses) = extracted {
            if responses.is_empty() {
                turns.push(content.clone());
            } else {
                function_responses.extend(responses);
            }
        } else {
            turns.push(content.clone());
        }
    }

    (turns, function_responses)
}

fn tool_response_message(
    function_responses: Vec<GeminiFunctionResponse>,
) -> Option<GeminiBidiGenerateContentClientMessage> {
    if function_responses.is_empty() {
        return None;
    }

    Some(GeminiBidiGenerateContentClientMessage {
        message_type: GeminiBidiGenerateContentClientMessageType::ToolResponse {
            tool_response: GeminiBidiGenerateContentToolResponse {
                function_responses: Some(function_responses),
            },
        },
    })
}

pub fn gemini_stream_request_to_live_frames_with_context(
    value: &GeminiStreamGenerateContentRequest,
) -> Result<
    (
        Vec<GeminiBidiGenerateContentClientMessage>,
        GeminiWebsocketTransformContext,
    ),
    TransformError,
> {
    let mut ctx = GeminiWebsocketTransformContext::default();
    let mut frames = vec![setup_message(value)];
    let (turns, function_responses) = split_turns_and_tool_responses(value, &mut ctx);
    if let Some(content) = content_message(turns) {
        frames.push(content);
    }
    if let Some(tool_response) = tool_response_message(function_responses) {
        frames.push(tool_response);
    }
    Ok((frames, ctx))
}

pub fn gemini_stream_request_to_live_connect_with_context(
    value: &GeminiStreamGenerateContentRequest,
) -> Result<(GeminiLiveConnectRequest, GeminiWebsocketTransformContext), TransformError> {
    Ok((
        GeminiLiveConnectRequest {
            body: Some(setup_message(value)),
            ..GeminiLiveConnectRequest::default()
        },
        GeminiWebsocketTransformContext::default(),
    ))
}

pub fn gemini_nonstream_request_to_live_frames_with_context(
    value: &GeminiGenerateContentRequest,
) -> Result<
    (
        Vec<GeminiBidiGenerateContentClientMessage>,
        GeminiWebsocketTransformContext,
    ),
    TransformError,
> {
    let stream_request = GeminiStreamGenerateContentRequest::try_from(value)?;
    gemini_stream_request_to_live_frames_with_context(&stream_request)
}

pub fn gemini_nonstream_request_to_live_connect_with_context(
    value: &GeminiGenerateContentRequest,
) -> Result<(GeminiLiveConnectRequest, GeminiWebsocketTransformContext), TransformError> {
    let stream_request = GeminiStreamGenerateContentRequest::try_from(value)?;
    gemini_stream_request_to_live_connect_with_context(&stream_request)
}

impl TryFrom<&GeminiStreamGenerateContentRequest> for Vec<GeminiBidiGenerateContentClientMessage> {
    type Error = TransformError;

    fn try_from(value: &GeminiStreamGenerateContentRequest) -> Result<Self, TransformError> {
        Ok(gemini_stream_request_to_live_frames_with_context(value)?.0)
    }
}

impl TryFrom<&GeminiStreamGenerateContentRequest> for GeminiLiveConnectRequest {
    type Error = TransformError;

    fn try_from(value: &GeminiStreamGenerateContentRequest) -> Result<Self, TransformError> {
        Ok(gemini_stream_request_to_live_connect_with_context(value)?.0)
    }
}

impl TryFrom<GeminiStreamGenerateContentRequest> for GeminiLiveConnectRequest {
    type Error = TransformError;

    fn try_from(value: GeminiStreamGenerateContentRequest) -> Result<Self, TransformError> {
        GeminiLiveConnectRequest::try_from(&value)
    }
}

impl TryFrom<&GeminiGenerateContentRequest> for Vec<GeminiBidiGenerateContentClientMessage> {
    type Error = TransformError;

    fn try_from(value: &GeminiGenerateContentRequest) -> Result<Self, TransformError> {
        Ok(gemini_nonstream_request_to_live_frames_with_context(value)?.0)
    }
}

impl TryFrom<&GeminiGenerateContentRequest> for GeminiLiveConnectRequest {
    type Error = TransformError;

    fn try_from(value: &GeminiGenerateContentRequest) -> Result<Self, TransformError> {
        Ok(gemini_nonstream_request_to_live_connect_with_context(value)?.0)
    }
}

impl TryFrom<GeminiGenerateContentRequest> for GeminiLiveConnectRequest {
    type Error = TransformError;

    fn try_from(value: GeminiGenerateContentRequest) -> Result<Self, TransformError> {
        GeminiLiveConnectRequest::try_from(&value)
    }
}

#[cfg(test)]
mod tests {
    use crate::gemini::count_tokens::types::{GeminiContent, GeminiPart};
    use crate::gemini::generate_content::request::GeminiGenerateContentRequest;
    use crate::gemini::stream_generate_content::request::{
        GeminiStreamGenerateContentRequest, PathParameters, RequestBody,
    };

    use super::*;

    #[test]
    fn stream_request_maps_to_setup_and_client_content_frames() {
        let request = GeminiStreamGenerateContentRequest {
            path: PathParameters {
                model: "gemini-2.5-flash".to_string(),
            },
            body: RequestBody {
                contents: vec![GeminiContent {
                    parts: vec![GeminiPart {
                        text: Some("hello".to_string()),
                        ..GeminiPart::default()
                    }],
                    role: None,
                }],
                ..RequestBody::default()
            },
            ..GeminiStreamGenerateContentRequest::default()
        };

        let frames = Vec::<GeminiBidiGenerateContentClientMessage>::try_from(&request)
            .expect("conversion should succeed");
        assert_eq!(frames.len(), 2);

        match &frames[0].message_type {
            GeminiBidiGenerateContentClientMessageType::Setup { setup } => {
                assert_eq!(setup.model, "models/gemini-2.5-flash");
            }
            _ => panic!("expected setup frame"),
        }

        match &frames[1].message_type {
            GeminiBidiGenerateContentClientMessageType::ClientContent { client_content } => {
                assert_eq!(client_content.turn_complete, Some(true));
            }
            _ => panic!("expected client content frame"),
        }
    }

    #[test]
    fn function_response_content_maps_to_tool_response_frame() {
        let request = GeminiStreamGenerateContentRequest {
            path: PathParameters {
                model: "gemini-2.5-flash".to_string(),
            },
            body: RequestBody {
                contents: vec![GeminiContent {
                    parts: vec![GeminiPart {
                        function_response: Some(
                            crate::gemini::count_tokens::types::GeminiFunctionResponse {
                                id: Some("call_1".to_string()),
                                name: "exec_command".to_string(),
                                response: Default::default(),
                                parts: None,
                                will_continue: None,
                                scheduling: None,
                            },
                        ),
                        ..GeminiPart::default()
                    }],
                    role: None,
                }],
                ..RequestBody::default()
            },
            ..GeminiStreamGenerateContentRequest::default()
        };

        let frames = Vec::<GeminiBidiGenerateContentClientMessage>::try_from(&request)
            .expect("conversion should succeed");
        assert_eq!(frames.len(), 2);
        assert!(matches!(
            frames[1].message_type,
            GeminiBidiGenerateContentClientMessageType::ToolResponse { .. }
        ));
    }

    #[test]
    fn nonstream_request_maps_to_live_frames_via_stream_bridge() {
        let request = GeminiGenerateContentRequest {
            path: crate::gemini::generate_content::request::PathParameters {
                model: "gemini-2.5-flash".to_string(),
            },
            body: crate::gemini::generate_content::request::RequestBody {
                contents: vec![GeminiContent {
                    parts: vec![GeminiPart {
                        text: Some("hello".to_string()),
                        ..GeminiPart::default()
                    }],
                    role: None,
                }],
                ..crate::gemini::generate_content::request::RequestBody::default()
            },
            ..GeminiGenerateContentRequest::default()
        };

        let frames = Vec::<GeminiBidiGenerateContentClientMessage>::try_from(&request)
            .expect("conversion should succeed");
        assert_eq!(frames.len(), 2);
    }
}
