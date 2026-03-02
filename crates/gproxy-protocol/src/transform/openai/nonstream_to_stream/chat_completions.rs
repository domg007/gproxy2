use crate::openai::create_chat_completions::response::OpenAiChatCompletionsResponse;
use crate::openai::create_chat_completions::stream::{
    ChatCompletionChunk, ChatCompletionChunkChoice, ChatCompletionChunkDelta,
    ChatCompletionChunkDeltaToolCall, ChatCompletionChunkDeltaToolCallType,
    ChatCompletionChunkObject, ChatCompletionFunctionCallDelta, OpenAiChatCompletionsSseData,
    OpenAiChatCompletionsSseEvent, OpenAiChatCompletionsSseStreamBody,
};
use crate::openai::create_chat_completions::types as ct;
use crate::transform::utils::TransformError;

fn to_stream_tool_call(
    tool_call: ct::ChatCompletionMessageToolCall,
    index: u32,
) -> ChatCompletionChunkDeltaToolCall {
    match tool_call {
        ct::ChatCompletionMessageToolCall::Function(call) => ChatCompletionChunkDeltaToolCall {
            index,
            id: Some(call.id),
            function: Some(ChatCompletionFunctionCallDelta {
                arguments: Some(call.function.arguments),
                name: Some(call.function.name),
            }),
            type_: Some(ChatCompletionChunkDeltaToolCallType::Function),
        },
        ct::ChatCompletionMessageToolCall::Custom(call) => ChatCompletionChunkDeltaToolCall {
            index,
            id: Some(call.id),
            function: Some(ChatCompletionFunctionCallDelta {
                arguments: Some(call.custom.input),
                name: Some(call.custom.name),
            }),
            type_: Some(ChatCompletionChunkDeltaToolCallType::Function),
        },
    }
}

impl TryFrom<OpenAiChatCompletionsResponse> for OpenAiChatCompletionsSseStreamBody {
    type Error = TransformError;

    fn try_from(value: OpenAiChatCompletionsResponse) -> Result<Self, TransformError> {
        match value {
            OpenAiChatCompletionsResponse::Success { body, .. } => {
                let mut events = Vec::new();

                for choice in body.choices {
                    let tool_calls = choice.message.tool_calls.map(|calls| {
                        calls
                            .into_iter()
                            .enumerate()
                            .map(|(tool_index, tool_call)| {
                                to_stream_tool_call(
                                    tool_call,
                                    u32::try_from(tool_index).unwrap_or(u32::MAX),
                                )
                            })
                            .collect::<Vec<_>>()
                    });

                    events.push(OpenAiChatCompletionsSseEvent {
                        event: None,
                        data: OpenAiChatCompletionsSseData::Chunk(ChatCompletionChunk {
                            id: body.id.clone(),
                            choices: vec![ChatCompletionChunkChoice {
                                delta: ChatCompletionChunkDelta {
                                    content: choice.message.content,
                                    function_call: choice
                                        .message
                                        .function_call
                                        .map(ChatCompletionFunctionCallDelta::from),
                                    refusal: choice.message.refusal,
                                    role: Some(ct::ChatCompletionDeltaRole::Assistant),
                                    annotations: choice.message.annotations,
                                    tool_calls,
                                    obfuscation: None,
                                },
                                finish_reason: Some(choice.finish_reason),
                                index: choice.index,
                                logprobs: choice.logprobs,
                            }],
                            created: body.created,
                            model: body.model.clone(),
                            object: ChatCompletionChunkObject::ChatCompletionChunk,
                            service_tier: body.service_tier.clone(),
                            system_fingerprint: body.system_fingerprint.clone(),
                            usage: None,
                        }),
                    });
                }

                if events.is_empty() {
                    events.push(OpenAiChatCompletionsSseEvent {
                        event: None,
                        data: OpenAiChatCompletionsSseData::Chunk(ChatCompletionChunk {
                            id: body.id.clone(),
                            choices: vec![ChatCompletionChunkChoice {
                                delta: ChatCompletionChunkDelta {
                                    role: Some(ct::ChatCompletionDeltaRole::Assistant),
                                    ..ChatCompletionChunkDelta::default()
                                },
                                finish_reason: Some(ct::ChatCompletionFinishReason::Stop),
                                index: 0,
                                logprobs: None,
                            }],
                            created: body.created,
                            model: body.model.clone(),
                            object: ChatCompletionChunkObject::ChatCompletionChunk,
                            service_tier: body.service_tier.clone(),
                            system_fingerprint: body.system_fingerprint.clone(),
                            usage: body.usage.clone(),
                        }),
                    });
                } else if let Some(usage) = body.usage.clone()
                    && let Some(OpenAiChatCompletionsSseEvent {
                        data: OpenAiChatCompletionsSseData::Chunk(chunk),
                        ..
                    }) = events.last_mut()
                {
                    chunk.usage = Some(usage);
                }

                events.push(OpenAiChatCompletionsSseEvent {
                    event: None,
                    data: OpenAiChatCompletionsSseData::Done("[DONE]".to_string()),
                });

                Ok(OpenAiChatCompletionsSseStreamBody { events })
            }
            OpenAiChatCompletionsResponse::Error { .. } => Err(TransformError::not_implemented(
                "cannot convert OpenAI chat error response to SSE stream body",
            )),
        }
    }
}
