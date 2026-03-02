use crate::openai::count_tokens::types as ot;
use crate::openai::create_chat_completions::request::{
    OpenAiChatCompletionsRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::openai::create_chat_completions::types as ct;
use crate::openai::create_response::request::OpenAiCreateResponseRequest;
use crate::openai::create_response::types::ResponsePromptCacheRetention;
use crate::transform::openai::count_tokens::openai::utils::{
    openai_function_call_output_content_to_text, openai_input_to_items,
    openai_message_content_to_text, openai_reasoning_summary_to_text,
};
use crate::transform::utils::TransformError;

use super::utils::{
    custom_call_output_to_text, message_content_to_user_content,
    response_reasoning_to_chat_reasoning, response_service_tier_to_chat,
    response_text_to_chat_response_format, response_text_to_chat_verbosity,
    response_tool_choice_to_chat_tool_choice, response_tools_to_chat_tools,
};

impl TryFrom<OpenAiCreateResponseRequest> for OpenAiChatCompletionsRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseRequest) -> Result<Self, TransformError> {
        let body = value.body;
        let mut messages = Vec::new();

        if let Some(instructions) = body.instructions.as_ref().filter(|text| !text.is_empty()) {
            messages.push(ct::ChatCompletionMessageParam::System(
                ct::ChatCompletionSystemMessageParam {
                    content: ct::ChatCompletionTextContent::Text(instructions.clone()),
                    role: ct::ChatCompletionSystemRole::System,
                    name: None,
                },
            ));
        }

        for item in openai_input_to_items(body.input.clone()) {
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
                    let text = format!("{other:?}");
                    messages.push(ct::ChatCompletionMessageParam::User(
                        ct::ChatCompletionUserMessageParam {
                            content: ct::ChatCompletionUserContent::Text(text),
                            role: ct::ChatCompletionUserRole::User,
                            name: None,
                        },
                    ));
                }
            }
        }

        let (tools, web_search_options) = response_tools_to_chat_tools(body.tools);

        Ok(OpenAiChatCompletionsRequest {
            method: ct::HttpMethod::Post,
            path: PathParameters::default(),
            query: QueryParameters::default(),
            headers: RequestHeaders::default(),
            body: RequestBody {
                messages,
                model: body.model.unwrap_or_default(),
                audio: None,
                frequency_penalty: None,
                function_call: None,
                functions: None,
                logit_bias: None,
                logprobs: None,
                max_completion_tokens: body.max_output_tokens,
                max_tokens: None,
                metadata: body.metadata,
                modalities: None,
                n: None,
                parallel_tool_calls: body.parallel_tool_calls,
                prediction: None,
                presence_penalty: None,
                prompt_cache_key: body.prompt_cache_key,
                prompt_cache_retention: body.prompt_cache_retention.map(|value| match value {
                    ResponsePromptCacheRetention::InMemory => {
                        ct::ChatCompletionPromptCacheRetention::InMemory
                    }
                    ResponsePromptCacheRetention::H24 => {
                        ct::ChatCompletionPromptCacheRetention::H24
                    }
                }),
                reasoning_effort: response_reasoning_to_chat_reasoning(body.reasoning),
                response_format: response_text_to_chat_response_format(body.text.as_ref()),
                safety_identifier: body.safety_identifier,
                seed: None,
                service_tier: response_service_tier_to_chat(body.service_tier),
                stop: None,
                store: body.store,
                stream: body.stream,
                stream_options: body.stream_options.map(|options| {
                    ct::ChatCompletionStreamOptions {
                        include_obfuscation: options.include_obfuscation,
                        include_usage: None,
                    }
                }),
                temperature: body.temperature,
                tool_choice: response_tool_choice_to_chat_tool_choice(body.tool_choice),
                tools,
                top_logprobs: body.top_logprobs,
                top_p: body.top_p,
                user: body.user,
                verbosity: response_text_to_chat_verbosity(body.text.as_ref()),
                extra_body: None,
                web_search_options,
            },
        })
    }
}
