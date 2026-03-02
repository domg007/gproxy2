use crate::openai::compact_response::request::OpenAiCompactRequest;
use crate::openai::count_tokens::types as ot;
use crate::openai::create_chat_completions::request::{
    OpenAiChatCompletionsRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::openai::create_chat_completions::types as ct;
use crate::transform::openai::compact::utils::{
    COMPACT_MAX_OUTPUT_TOKENS, compact_system_instruction,
};
use crate::transform::openai::count_tokens::openai::utils::{
    openai_function_call_output_content_to_text, openai_input_to_items,
    openai_message_content_to_text, openai_reasoning_summary_to_text,
};
use crate::transform::openai::generate_content::openai_response::openai_chat_completions::utils::{
    custom_call_output_to_text, message_content_to_user_content,
};
use crate::transform::utils::TransformError;

impl TryFrom<OpenAiCompactRequest> for OpenAiChatCompletionsRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCompactRequest) -> Result<Self, TransformError> {
        let body = value.body;
        let mut messages = vec![ct::ChatCompletionMessageParam::System(
            ct::ChatCompletionSystemMessageParam {
                content: ct::ChatCompletionTextContent::Text(compact_system_instruction(
                    body.instructions,
                )),
                role: ct::ChatCompletionSystemRole::System,
                name: None,
            },
        )];

        for (index, item) in openai_input_to_items(body.input).into_iter().enumerate() {
            match item {
                ot::ResponseInputItem::Message(message) => match message.role {
                    ot::ResponseInputMessageRole::User => {
                        messages.push(ct::ChatCompletionMessageParam::User(
                            ct::ChatCompletionUserMessageParam {
                                content: message_content_to_user_content(message.content),
                                role: ct::ChatCompletionUserRole::User,
                                name: None,
                            },
                        ));
                    }
                    ot::ResponseInputMessageRole::Assistant => {
                        let text = openai_message_content_to_text(&message.content);
                        messages.push(ct::ChatCompletionMessageParam::Assistant(
                            ct::ChatCompletionAssistantMessageParam {
                                role: ct::ChatCompletionAssistantRole::Assistant,
                                audio: None,
                                content: if text.is_empty() {
                                    None
                                } else {
                                    Some(ct::ChatCompletionAssistantContent::Text(text))
                                },
                                reasoning_content: None,
                                function_call: None,
                                name: None,
                                refusal: None,
                                tool_calls: None,
                            },
                        ));
                    }
                    ot::ResponseInputMessageRole::System => {
                        let text = openai_message_content_to_text(&message.content);
                        messages.push(ct::ChatCompletionMessageParam::System(
                            ct::ChatCompletionSystemMessageParam {
                                content: ct::ChatCompletionTextContent::Text(text),
                                role: ct::ChatCompletionSystemRole::System,
                                name: None,
                            },
                        ));
                    }
                    ot::ResponseInputMessageRole::Developer => {
                        let text = openai_message_content_to_text(&message.content);
                        messages.push(ct::ChatCompletionMessageParam::Developer(
                            ct::ChatCompletionDeveloperMessageParam {
                                content: ct::ChatCompletionTextContent::Text(text),
                                role: ct::ChatCompletionDeveloperRole::Developer,
                                name: None,
                            },
                        ));
                    }
                },
                ot::ResponseInputItem::OutputMessage(message) => {
                    let mut text_parts = Vec::new();
                    let mut refusal_parts = Vec::new();
                    for part in message.content {
                        match part {
                            ot::ResponseOutputContent::Text(text) => {
                                if !text.text.is_empty() {
                                    text_parts.push(text.text);
                                }
                            }
                            ot::ResponseOutputContent::Refusal(refusal) => {
                                if !refusal.refusal.is_empty() {
                                    refusal_parts.push(refusal.refusal);
                                }
                            }
                        }
                    }

                    messages.push(ct::ChatCompletionMessageParam::Assistant(
                        ct::ChatCompletionAssistantMessageParam {
                            role: ct::ChatCompletionAssistantRole::Assistant,
                            audio: None,
                            content: if text_parts.is_empty() {
                                None
                            } else {
                                Some(ct::ChatCompletionAssistantContent::Text(
                                    text_parts.join("\n"),
                                ))
                            },
                            reasoning_content: None,
                            function_call: None,
                            name: None,
                            refusal: if refusal_parts.is_empty() {
                                None
                            } else {
                                Some(refusal_parts.join("\n"))
                            },
                            tool_calls: None,
                        },
                    ));
                }
                ot::ResponseInputItem::FunctionToolCall(tool_call) => {
                    messages.push(ct::ChatCompletionMessageParam::Assistant(
                        ct::ChatCompletionAssistantMessageParam {
                            role: ct::ChatCompletionAssistantRole::Assistant,
                            audio: None,
                            content: None,
                            reasoning_content: None,
                            function_call: None,
                            name: None,
                            refusal: None,
                            tool_calls: Some(vec![ct::ChatCompletionMessageToolCall::Function(
                                ct::ChatCompletionMessageFunctionToolCall {
                                    id: tool_call.call_id,
                                    function: ct::ChatCompletionFunctionCall {
                                        arguments: tool_call.arguments,
                                        name: tool_call.name,
                                    },
                                    type_: ct::ChatCompletionMessageFunctionToolCallType::Function,
                                },
                            )]),
                        },
                    ));
                }
                ot::ResponseInputItem::CustomToolCall(tool_call) => {
                    let id = tool_call
                        .id
                        .clone()
                        .unwrap_or_else(|| tool_call.call_id.clone());
                    messages.push(ct::ChatCompletionMessageParam::Assistant(
                        ct::ChatCompletionAssistantMessageParam {
                            role: ct::ChatCompletionAssistantRole::Assistant,
                            audio: None,
                            content: None,
                            reasoning_content: None,
                            function_call: None,
                            name: None,
                            refusal: None,
                            tool_calls: Some(vec![ct::ChatCompletionMessageToolCall::Custom(
                                ct::ChatCompletionMessageCustomToolCall {
                                    id,
                                    custom: ct::ChatCompletionMessageCustomToolCallPayload {
                                        input: tool_call.input,
                                        name: tool_call.name,
                                    },
                                    type_: ct::ChatCompletionMessageCustomToolCallType::Custom,
                                },
                            )]),
                        },
                    ));
                }
                ot::ResponseInputItem::FunctionCallOutput(call) => {
                    messages.push(ct::ChatCompletionMessageParam::Tool(
                        ct::ChatCompletionToolMessageParam {
                            content: ct::ChatCompletionTextContent::Text(
                                openai_function_call_output_content_to_text(&call.output),
                            ),
                            role: ct::ChatCompletionToolRole::Tool,
                            tool_call_id: call.call_id,
                        },
                    ));
                }
                ot::ResponseInputItem::CustomToolCallOutput(call) => {
                    messages.push(ct::ChatCompletionMessageParam::Tool(
                        ct::ChatCompletionToolMessageParam {
                            content: ct::ChatCompletionTextContent::Text(
                                custom_call_output_to_text(&call.output),
                            ),
                            role: ct::ChatCompletionToolRole::Tool,
                            tool_call_id: call.call_id,
                        },
                    ));
                }
                ot::ResponseInputItem::ReasoningItem(reasoning) => {
                    let mut text = openai_reasoning_summary_to_text(&reasoning.summary);
                    if text.is_empty()
                        && let Some(content) = reasoning.encrypted_content
                    {
                        text = content;
                    }
                    if !text.is_empty() {
                        messages.push(ct::ChatCompletionMessageParam::Assistant(
                            ct::ChatCompletionAssistantMessageParam {
                                role: ct::ChatCompletionAssistantRole::Assistant,
                                audio: None,
                                content: Some(ct::ChatCompletionAssistantContent::Text(text)),
                                reasoning_content: None,
                                function_call: None,
                                name: None,
                                refusal: None,
                                tool_calls: None,
                            },
                        ));
                    }
                }
                other => {
                    messages.push(ct::ChatCompletionMessageParam::User(
                        ct::ChatCompletionUserMessageParam {
                            content: ct::ChatCompletionUserContent::Text(format!("{other:?}")),
                            role: ct::ChatCompletionUserRole::User,
                            name: Some(format!("compact_item_{index}")),
                        },
                    ));
                }
            }
        }

        Ok(OpenAiChatCompletionsRequest {
            method: ct::HttpMethod::Post,
            path: PathParameters::default(),
            query: QueryParameters::default(),
            headers: RequestHeaders::default(),
            body: RequestBody {
                messages,
                model: body.model,
                max_completion_tokens: Some(COMPACT_MAX_OUTPUT_TOKENS),
                max_tokens: Some(COMPACT_MAX_OUTPUT_TOKENS),
                ..RequestBody::default()
            },
        })
    }
}
