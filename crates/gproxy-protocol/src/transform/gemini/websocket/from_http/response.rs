use crate::gemini::count_tokens::types::GeminiContent;
use crate::gemini::generate_content::response::ResponseBody as GeminiGenerateContentResponseBody;
use crate::gemini::generate_content::types::{GeminiCandidate, GeminiUsageMetadata};
use crate::gemini::live::response::GeminiLiveMessageResponse;
use crate::gemini::live::types::{
    GeminiBidiGenerateContentServerContent, GeminiBidiGenerateContentServerMessage,
    GeminiBidiGenerateContentServerMessageType, GeminiBidiGenerateContentToolCall,
    GeminiFunctionCall, GeminiLiveUsageMetadata,
};
use crate::gemini::stream_generate_content::response::GeminiStreamGenerateContentResponse;
use crate::gemini::stream_generate_content::stream::GeminiSseEventData;
use crate::transform::gemini::websocket::context::GeminiWebsocketTransformContext;
use crate::transform::utils::TransformError;

fn usage_generate_to_live(usage: Option<GeminiUsageMetadata>) -> Option<GeminiLiveUsageMetadata> {
    usage.map(|usage| GeminiLiveUsageMetadata {
        prompt_token_count: usage.prompt_token_count,
        cached_content_token_count: usage.cached_content_token_count,
        response_token_count: usage.candidates_token_count,
        tool_use_prompt_token_count: usage.tool_use_prompt_token_count,
        thoughts_token_count: usage.thoughts_token_count,
        total_token_count: usage.total_token_count,
        prompt_tokens_details: usage.prompt_tokens_details,
        cache_tokens_details: usage.cache_tokens_details,
        response_tokens_details: usage.candidates_tokens_details,
        tool_use_prompt_tokens_details: usage.tool_use_prompt_tokens_details,
    })
}

fn candidate_to_server_message(
    candidate: GeminiCandidate,
    usage_metadata: Option<GeminiLiveUsageMetadata>,
) -> Option<GeminiLiveMessageResponse> {
    let generation_complete = candidate.finish_reason.is_some();

    let has_payload = candidate.content.is_some()
        || candidate.finish_reason.is_some()
        || candidate.grounding_metadata.is_some()
        || candidate.url_context_metadata.is_some()
        || usage_metadata.is_some();

    if !has_payload {
        return None;
    }

    let as_tool_calls = candidate
        .content
        .as_ref()
        .and_then(content_as_pure_function_calls);

    Some(GeminiLiveMessageResponse::Message(match as_tool_calls {
        Some(function_calls) => GeminiBidiGenerateContentServerMessage {
            usage_metadata,
            message_type: GeminiBidiGenerateContentServerMessageType::ToolCall {
                tool_call: GeminiBidiGenerateContentToolCall {
                    function_calls: Some(function_calls),
                },
            },
        },
        None => GeminiBidiGenerateContentServerMessage {
            usage_metadata,
            message_type: GeminiBidiGenerateContentServerMessageType::ServerContent {
                server_content: GeminiBidiGenerateContentServerContent {
                    generation_complete: generation_complete.then_some(true),
                    turn_complete: generation_complete.then_some(true),
                    interrupted: None,
                    grounding_metadata: candidate.grounding_metadata,
                    input_transcription: None,
                    output_transcription: None,
                    url_context_metadata: candidate.url_context_metadata,
                    model_turn: candidate.content,
                },
            },
        },
    }))
}

fn content_as_pure_function_calls(content: &GeminiContent) -> Option<Vec<GeminiFunctionCall>> {
    let mut calls = Vec::new();
    for part in &content.parts {
        let call = part.function_call.clone()?;
        let has_non_call_fields = part.text.is_some()
            || part.inline_data.is_some()
            || part.function_response.is_some()
            || part.file_data.is_some()
            || part.executable_code.is_some()
            || part.code_execution_result.is_some();
        if has_non_call_fields {
            return None;
        }
        calls.push(call);
    }

    if calls.is_empty() { None } else { Some(calls) }
}

fn chunk_to_live_messages(
    chunk: GeminiGenerateContentResponseBody,
    ctx: &mut GeminiWebsocketTransformContext,
) -> Vec<GeminiLiveMessageResponse> {
    if chunk.prompt_feedback.is_some() {
        ctx.push_warning("gemini websocket from_http response: dropped promptFeedback".to_string());
    }
    if chunk.model_version.is_some() {
        ctx.push_warning("gemini websocket from_http response: dropped modelVersion".to_string());
    }
    if chunk.response_id.is_some() {
        ctx.push_warning("gemini websocket from_http response: dropped responseId".to_string());
    }
    if chunk.model_status.is_some() {
        ctx.push_warning("gemini websocket from_http response: dropped modelStatus".to_string());
    }

    let usage_metadata = usage_generate_to_live(chunk.usage_metadata);

    let mut messages = Vec::new();
    if let Some(candidates) = chunk.candidates {
        for candidate in candidates {
            if let Some(message) = candidate_to_server_message(candidate, usage_metadata.clone()) {
                messages.push(message);
            }
        }
    }

    if messages.is_empty() && usage_metadata.is_some() {
        messages.push(GeminiLiveMessageResponse::Message(
            GeminiBidiGenerateContentServerMessage {
                usage_metadata,
                message_type: GeminiBidiGenerateContentServerMessageType::ServerContent {
                    server_content: GeminiBidiGenerateContentServerContent::default(),
                },
            },
        ));
    }

    messages
}

impl TryFrom<GeminiStreamGenerateContentResponse> for Vec<GeminiLiveMessageResponse> {
    type Error = TransformError;

    fn try_from(value: GeminiStreamGenerateContentResponse) -> Result<Self, TransformError> {
        Ok(gemini_stream_response_to_live_messages_with_context(value)?.0)
    }
}

pub fn gemini_stream_response_to_live_messages_with_context(
    value: GeminiStreamGenerateContentResponse,
) -> Result<
    (
        Vec<GeminiLiveMessageResponse>,
        GeminiWebsocketTransformContext,
    ),
    TransformError,
> {
    let mut ctx = GeminiWebsocketTransformContext::default();
    let mut out = Vec::new();
    match value {
        GeminiStreamGenerateContentResponse::NdjsonSuccess { body, .. } => {
            for chunk in body.chunks {
                out.extend(chunk_to_live_messages(chunk, &mut ctx));
            }
        }
        GeminiStreamGenerateContentResponse::SseSuccess { body, .. } => {
            for event in body.events {
                if let GeminiSseEventData::Chunk(chunk) = event.data {
                    out.extend(chunk_to_live_messages(chunk, &mut ctx));
                }
            }
        }
        GeminiStreamGenerateContentResponse::Error { body, .. } => {
            out.push(GeminiLiveMessageResponse::Error(body));
        }
    }

    Ok((out, ctx))
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use crate::gemini::count_tokens::types::{GeminiContent, GeminiPart};
    use crate::gemini::generate_content::response::ResponseBody;
    use crate::gemini::generate_content::types::{GeminiCandidate, GeminiFinishReason};
    use crate::gemini::stream_generate_content::response::GeminiStreamGenerateContentResponse;
    use crate::gemini::stream_generate_content::stream::GeminiNdjsonStreamBody;
    use crate::gemini::types::GeminiResponseHeaders;

    use super::*;

    #[test]
    fn stream_response_chunk_maps_to_live_server_content() {
        let response = GeminiStreamGenerateContentResponse::NdjsonSuccess {
            stats_code: StatusCode::OK,
            headers: GeminiResponseHeaders::default(),
            body: GeminiNdjsonStreamBody {
                chunks: vec![ResponseBody {
                    candidates: Some(vec![GeminiCandidate {
                        content: Some(GeminiContent {
                            parts: vec![GeminiPart {
                                text: Some("hello".to_string()),
                                ..GeminiPart::default()
                            }],
                            role: None,
                        }),
                        finish_reason: Some(GeminiFinishReason::Stop),
                        ..GeminiCandidate::default()
                    }]),
                    ..ResponseBody::default()
                }],
            },
        };

        let frames = Vec::<GeminiLiveMessageResponse>::try_from(response)
            .expect("conversion should succeed");
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn function_call_candidate_maps_to_tool_call() {
        let response = GeminiStreamGenerateContentResponse::NdjsonSuccess {
            stats_code: StatusCode::OK,
            headers: GeminiResponseHeaders::default(),
            body: GeminiNdjsonStreamBody {
                chunks: vec![ResponseBody {
                    candidates: Some(vec![GeminiCandidate {
                        content: Some(GeminiContent {
                            parts: vec![GeminiPart {
                                function_call: Some(GeminiFunctionCall {
                                    id: Some("call_1".to_string()),
                                    name: "exec_command".to_string(),
                                    args: None,
                                }),
                                ..GeminiPart::default()
                            }],
                            role: None,
                        }),
                        ..GeminiCandidate::default()
                    }]),
                    ..ResponseBody::default()
                }],
            },
        };

        let frames = Vec::<GeminiLiveMessageResponse>::try_from(response)
            .expect("conversion should succeed");
        assert!(matches!(
            frames[0],
            GeminiLiveMessageResponse::Message(GeminiBidiGenerateContentServerMessage {
                message_type: GeminiBidiGenerateContentServerMessageType::ToolCall { .. },
                ..
            })
        ));
    }
}
