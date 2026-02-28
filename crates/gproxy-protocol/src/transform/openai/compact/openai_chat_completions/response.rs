use crate::openai::compact_response::response::OpenAiCompactResponse;
use crate::openai::compact_response::response::{
    OpenAiCompactedResponseObject, ResponseBody as CompactResponseBody,
};
use crate::openai::compact_response::types as cpt;
use crate::openai::count_tokens::types as ot;
use crate::openai::create_chat_completions::response::OpenAiChatCompletionsResponse;
use crate::transform::utils::TransformError;

impl TryFrom<OpenAiChatCompletionsResponse> for OpenAiCompactResponse {
    type Error = TransformError;

    fn try_from(value: OpenAiChatCompletionsResponse) -> Result<Self, TransformError> {
        Ok(match value {
            OpenAiChatCompletionsResponse::Success {
                stats_code,
                headers,
                body,
            } => {
                let mut output = Vec::new();

                for choice in body.choices {
                    let annotations = choice
                        .message
                        .annotations
                        .unwrap_or_default()
                        .into_iter()
                        .map(|annotation| {
                            ot::ResponseOutputTextAnnotation::UrlCitation(ot::ResponseUrlCitation {
                                end_index: annotation.url_citation.end_index,
                                start_index: annotation.url_citation.start_index,
                                title: annotation.url_citation.title,
                                type_: ot::ResponseUrlCitationType::UrlCitation,
                                url: annotation.url_citation.url,
                            })
                        })
                        .collect::<Vec<_>>();

                    let output_logprobs = choice.logprobs.and_then(|value| {
                        value.content.map(|content| {
                            content
                                .into_iter()
                                .map(|entry| ot::ResponseOutputTokenLogprob {
                                    token: entry.token,
                                    bytes: entry.bytes,
                                    logprob: entry.logprob,
                                    top_logprobs: entry
                                        .top_logprobs
                                        .into_iter()
                                        .map(|top| ot::ResponseOutputTopLogprob {
                                            token: top.token,
                                            bytes: top.bytes,
                                            logprob: top.logprob,
                                        })
                                        .collect::<Vec<_>>(),
                                })
                                .collect::<Vec<_>>()
                        })
                    });

                    let mut message_content = Vec::new();
                    if let Some(text) = choice.message.content
                        && !text.is_empty()
                    {
                        message_content.push(cpt::CompactedResponseMessageContent::OutputText(
                            ot::ResponseOutputText {
                                annotations,
                                logprobs: output_logprobs,
                                text,
                                type_: ot::ResponseOutputTextType::OutputText,
                            },
                        ));
                    }
                    if let Some(refusal) = choice.message.refusal
                        && !refusal.is_empty()
                    {
                        message_content.push(cpt::CompactedResponseMessageContent::Refusal(
                            ot::ResponseOutputRefusal {
                                refusal,
                                type_: ot::ResponseOutputRefusalType::Refusal,
                            },
                        ));
                    }

                    if !message_content.is_empty() {
                        let status = match choice.finish_reason {
                            crate::openai::create_chat_completions::types::ChatCompletionFinishReason::Length
                            | crate::openai::create_chat_completions::types::ChatCompletionFinishReason::ContentFilter => ot::ResponseItemStatus::Incomplete,
                            _ => ot::ResponseItemStatus::Completed,
                        };
                        output.push(cpt::CompactedResponseOutputItem::Message(
                            cpt::CompactedResponseMessage {
                                id: format!("{}_message_{}", body.id, choice.index),
                                content: message_content,
                                role: cpt::CompactedResponseMessageRole::Assistant,
                                status,
                                type_: cpt::CompactedResponseMessageType::Message,
                            },
                        ));
                    }

                    if let Some(function_call) = choice.message.function_call {
                        output.push(cpt::CompactedResponseOutputItem::FunctionToolCall(
                            ot::ResponseFunctionToolCall {
                                arguments: function_call.arguments,
                                call_id: format!("{}_function_call_{}", body.id, choice.index),
                                name: function_call.name,
                                type_: ot::ResponseFunctionToolCallType::FunctionCall,
                                id: None,
                                status: Some(ot::ResponseItemStatus::Completed),
                            },
                        ));
                    }

                    if let Some(tool_calls) = choice.message.tool_calls {
                        for call in tool_calls {
                            match call {
                                crate::openai::create_chat_completions::types::ChatCompletionMessageToolCall::Function(call) => {
                                    output.push(cpt::CompactedResponseOutputItem::FunctionToolCall(
                                        ot::ResponseFunctionToolCall {
                                            arguments: call.function.arguments,
                                            call_id: call.id.clone(),
                                            name: call.function.name,
                                            type_: ot::ResponseFunctionToolCallType::FunctionCall,
                                            id: Some(call.id),
                                            status: Some(ot::ResponseItemStatus::Completed),
                                        },
                                    ));
                                }
                                crate::openai::create_chat_completions::types::ChatCompletionMessageToolCall::Custom(call) => {
                                    output.push(cpt::CompactedResponseOutputItem::CustomToolCall(
                                        ot::ResponseCustomToolCall {
                                            call_id: call.id.clone(),
                                            input: call.custom.input,
                                            name: call.custom.name,
                                            type_: ot::ResponseCustomToolCallType::CustomToolCall,
                                            id: Some(call.id),
                                        },
                                    ));
                                }
                            }
                        }
                    }
                }

                let usage = body
                    .usage
                    .map(|usage| cpt::ResponseUsage {
                        input_tokens: usage.prompt_tokens,
                        input_tokens_details: cpt::ResponseInputTokensDetails {
                            cached_tokens: usage
                                .prompt_tokens_details
                                .and_then(|details| details.cached_tokens)
                                .unwrap_or(0),
                        },
                        output_tokens: usage.completion_tokens,
                        output_tokens_details: cpt::ResponseOutputTokensDetails {
                            reasoning_tokens: usage
                                .completion_tokens_details
                                .and_then(|details| details.reasoning_tokens)
                                .unwrap_or(0),
                        },
                        total_tokens: usage.total_tokens,
                    })
                    .unwrap_or(cpt::ResponseUsage {
                        input_tokens: 0,
                        input_tokens_details: cpt::ResponseInputTokensDetails { cached_tokens: 0 },
                        output_tokens: 0,
                        output_tokens_details: cpt::ResponseOutputTokensDetails {
                            reasoning_tokens: 0,
                        },
                        total_tokens: 0,
                    });

                OpenAiCompactResponse::Success {
                    stats_code,
                    headers,
                    body: CompactResponseBody {
                        id: body.id,
                        created_at: body.created,
                        object: OpenAiCompactedResponseObject::ResponseCompaction,
                        output,
                        usage,
                    },
                }
            }
            OpenAiChatCompletionsResponse::Error {
                stats_code,
                headers,
                body,
            } => OpenAiCompactResponse::Error {
                stats_code,
                headers,
                body,
            },
        })
    }
}
