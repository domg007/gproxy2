//! OpenAI Responses -> OpenAI Chat Completions transforms.

use crate::protocol::openai::*;
use crate::transform::{TransformContext, TransformError};

pub fn request(
    input: ResponseCreateRequest,
    ctx: &TransformContext,
) -> Result<ChatCompletionRequest, TransformError> {
    reject_some(&input.background, "background")?;
    reject_some(&input.context_management, "context_management")?;
    reject_some(&input.conversation, "conversation")?;
    reject_some(&input.include, "include")?;
    reject_some(&input.max_tool_calls, "max_tool_calls")?;
    reject_some(&input.previous_response_id, "previous_response_id")?;
    reject_some(&input.prompt, "prompt")?;
    reject_some(
        &input.text.as_ref().and_then(|text| text.format.as_ref()),
        "text.format",
    )?;
    reject_some(&input.tool_choice, "tool_choice")?;
    reject_some(&input.tools, "tools")?;
    reject_some(&input.truncation, "truncation")?;

    let Some(model) = input.model else {
        return Err(TransformError::InvalidInput {
            reason: "OpenAI Chat request requires `model`".to_owned(),
        });
    };

    let mut messages = Vec::new();
    if let Some(instructions) = input.instructions {
        messages.push(ChatCompletionMessageParam::Developer {
            content: ChatTextContent::Text(instructions),
            name: None,
            extra: Extra::new(),
        });
    }
    if let Some(input) = input.input {
        append_response_input(input, &mut messages, ctx)?;
    }

    let stream_options = input
        .stream_options
        .map(|options| {
            let extra = carry_extra(options.extra, ctx, "stream_options")?;
            Ok(StreamOptions {
                include_obfuscation: options.include_obfuscation,
                include_usage: None,
                extra,
            })
        })
        .transpose()?;

    let (reasoning_effort, reasoning_extra) = match input.reasoning {
        Some(reasoning) => {
            reject_some(&reasoning.summary, "reasoning.summary")?;
            reject_some(&reasoning.generate_summary, "reasoning.generate_summary")?;
            (reasoning.effort, reasoning.extra)
        }
        None => (None, Extra::new()),
    };
    carry_extra(reasoning_extra, ctx, "reasoning")?;

    let verbosity = match input.text {
        Some(text) => {
            let extra = carry_extra(text.extra, ctx, "text")?;
            drop(extra);
            text.verbosity
        }
        None => None,
    };

    Ok(ChatCompletionRequest {
        messages,
        model,
        audio: None,
        frequency_penalty: None,
        function_call: None,
        functions: None,
        logit_bias: None,
        logprobs: None,
        max_completion_tokens: input.max_output_tokens,
        max_tokens: None,
        metadata: input.metadata,
        modalities: None,
        moderation: input.moderation,
        n: None,
        parallel_tool_calls: input.parallel_tool_calls,
        prediction: None,
        presence_penalty: None,
        prompt_cache_key: input.prompt_cache_key,
        prompt_cache_retention: input.prompt_cache_retention,
        reasoning_effort,
        response_format: None,
        safety_identifier: input.safety_identifier,
        seed: None,
        service_tier: input.service_tier,
        stop: None,
        store: input.store,
        stream: input.stream,
        stream_options,
        temperature: input.temperature,
        tool_choice: None,
        tools: None,
        top_logprobs: input.top_logprobs,
        top_p: input.top_p,
        user: input.user,
        verbosity,
        web_search_options: None,
        extra: carry_extra(input.extra, ctx, "request")?,
    })
}

fn append_response_input(
    input: ResponseInput,
    messages: &mut Vec<ChatCompletionMessageParam>,
    ctx: &TransformContext,
) -> Result<(), TransformError> {
    match input {
        ResponseInput::Text(text) => {
            messages.push(ChatCompletionMessageParam::User {
                content: ChatContent::Text(text),
                name: None,
                extra: Extra::new(),
            });
        }
        ResponseInput::Items(items) => {
            for item in items {
                messages.push(response_item_to_chat_message(item, ctx)?);
            }
        }
    }

    Ok(())
}

fn response_item_to_chat_message(
    item: ResponseItem,
    ctx: &TransformContext,
) -> Result<ChatCompletionMessageParam, TransformError> {
    match item {
        ResponseItem::Message(ResponseMessageItem::EasyInput(message)) => {
            reject_some(&message.phase, "input.phase")?;
            easy_message_to_chat_message(message)
        }
        ResponseItem::Message(ResponseMessageItem::Input(message)) => {
            reject_some(&message.id, "input.id")?;
            reject_some(&message.status, "input.status")?;
            input_message_to_chat_message(message)
        }
        ResponseItem::Message(ResponseMessageItem::Output(message)) => {
            output_message_to_chat_message(message, ctx)
        }
        ResponseItem::Typed(_) => Err(TransformError::UnsupportedField {
            field: "input",
            reason: "typed response items need pair-specific tool mapping",
        }),
        ResponseItem::Unknown(_) => Err(TransformError::UnsupportedField {
            field: "input",
            reason: "unknown response items cannot be mapped to chat messages",
        }),
    }
}

fn easy_message_to_chat_message(
    message: ResponseEasyInputMessageItem,
) -> Result<ChatCompletionMessageParam, TransformError> {
    let content = match message.content {
        ResponseEasyInputContent::Text(text) => text,
        ResponseEasyInputContent::Parts(parts) => response_input_parts_to_text(parts)?,
    };
    let extra = message.extra;

    match message.role {
        ResponseEasyInputMessageRole::Developer => Ok(ChatCompletionMessageParam::Developer {
            content: ChatTextContent::Text(content),
            name: None,
            extra,
        }),
        ResponseEasyInputMessageRole::System => Ok(ChatCompletionMessageParam::System {
            content: ChatTextContent::Text(content),
            name: None,
            extra,
        }),
        ResponseEasyInputMessageRole::User => Ok(ChatCompletionMessageParam::User {
            content: ChatContent::Text(content),
            name: None,
            extra,
        }),
        ResponseEasyInputMessageRole::Assistant => Ok(ChatCompletionMessageParam::Assistant {
            content: Some(ChatAssistantContent::Text(content)),
            audio: None,
            function_call: None,
            name: None,
            refusal: None,
            tool_calls: None,
            extra,
        }),
    }
}

fn input_message_to_chat_message(
    message: ResponseInputMessageItem,
) -> Result<ChatCompletionMessageParam, TransformError> {
    let extra = message.extra;
    match message.role {
        ResponseInputMessageRole::Developer => Ok(ChatCompletionMessageParam::Developer {
            content: ChatTextContent::Text(response_input_parts_to_text(message.content)?),
            name: None,
            extra,
        }),
        ResponseInputMessageRole::System => Ok(ChatCompletionMessageParam::System {
            content: ChatTextContent::Text(response_input_parts_to_text(message.content)?),
            name: None,
            extra,
        }),
        ResponseInputMessageRole::User => Ok(ChatCompletionMessageParam::User {
            content: ChatContent::Parts(response_input_parts_to_chat_parts(message.content)?),
            name: None,
            extra,
        }),
    }
}

fn output_message_to_chat_message(
    message: ResponseOutputMessageItem,
    ctx: &TransformContext,
) -> Result<ChatCompletionMessageParam, TransformError> {
    reject_some(&message.phase, "output.phase")?;

    let mut parts = Vec::new();
    let mut refusal = None;
    for part in message.content {
        match part {
            ResponseMessageOutputContentPart::OutputText { text, extra, .. } => {
                carry_extra(extra, ctx, "output_text")?;
                parts.push(ChatAssistantContentPart::Text {
                    text,
                    extra: Extra::new(),
                });
            }
            ResponseMessageOutputContentPart::Refusal {
                refusal: value,
                extra,
            } => {
                carry_extra(extra, ctx, "refusal")?;
                refusal = Some(value.clone());
                parts.push(ChatAssistantContentPart::Refusal {
                    refusal: value,
                    extra: Extra::new(),
                });
            }
        }
    }

    Ok(ChatCompletionMessageParam::Assistant {
        content: Some(ChatAssistantContent::Parts(parts)),
        audio: None,
        function_call: None,
        name: None,
        refusal,
        tool_calls: None,
        extra: message.extra,
    })
}

fn response_input_parts_to_text(
    parts: Vec<ResponseInputContentPart>,
) -> Result<String, TransformError> {
    let mut text = String::new();
    for part in parts {
        match part {
            ResponseInputContentPart::InputText { text: value, extra } => {
                require_empty_extra(extra, "input_text")?;
                text.push_str(&value);
            }
            _ => {
                return Err(TransformError::UnsupportedField {
                    field: "content",
                    reason: "non-text content cannot be mapped to system/developer text content",
                });
            }
        }
    }
    Ok(text)
}

fn response_input_parts_to_chat_parts(
    parts: Vec<ResponseInputContentPart>,
) -> Result<Vec<ChatContentPart>, TransformError> {
    parts
        .into_iter()
        .map(|part| match part {
            ResponseInputContentPart::InputText { text, extra } => {
                Ok(ChatContentPart::Text { text, extra })
            }
            ResponseInputContentPart::InputImage {
                detail,
                image_url: Some(url),
                extra,
                ..
            } => Ok(ChatContentPart::ImageUrl {
                image_url: ImageUrl {
                    url,
                    detail: detail.map(response_detail_to_chat_detail).transpose()?,
                    extra: Extra::new(),
                },
                extra,
            }),
            ResponseInputContentPart::InputAudio { input_audio, extra } => {
                Ok(ChatContentPart::InputAudio {
                    input_audio: InputAudio {
                        data: input_audio.data,
                        format: input_audio.format,
                        extra: input_audio.extra,
                    },
                    extra,
                })
            }
            ResponseInputContentPart::InputFile {
                file_data,
                file_id,
                filename,
                extra,
                ..
            } => Ok(ChatContentPart::File {
                file: ChatFileRef {
                    file_data,
                    file_id,
                    filename,
                    extra: Extra::new(),
                },
                extra,
            }),
            _ => Err(TransformError::UnsupportedField {
                field: "content",
                reason: "input image without image_url cannot be mapped to chat image_url",
            }),
        })
        .collect()
}

fn response_detail_to_chat_detail(
    detail: DetailLevel,
) -> Result<ChatImageDetailLevel, TransformError> {
    match detail {
        DetailLevel::Low => Ok(ChatImageDetailLevel::Low),
        DetailLevel::High => Ok(ChatImageDetailLevel::High),
        DetailLevel::Auto => Ok(ChatImageDetailLevel::Auto),
        DetailLevel::Original => Err(TransformError::UnsupportedField {
            field: "detail",
            reason: "OpenAI Chat image detail does not support `original`",
        }),
    }
}

fn reject_some<T>(value: &Option<T>, field: &'static str) -> Result<(), TransformError> {
    if value.is_some() {
        return Err(TransformError::UnsupportedField {
            field,
            reason: "field has no safe OpenAI Chat equivalent in this transform",
        });
    }
    Ok(())
}

fn carry_extra(
    extra: Extra,
    ctx: &TransformContext,
    field: &'static str,
) -> Result<Extra, TransformError> {
    if ctx.preserve_unknown_fields || extra.is_empty() {
        return Ok(extra);
    }
    Err(TransformError::LossyField {
        field,
        reason: "extra fields would be dropped",
    })
}

fn require_empty_extra(extra: Extra, field: &'static str) -> Result<(), TransformError> {
    if extra.is_empty() {
        return Ok(());
    }
    Err(TransformError::LossyField {
        field,
        reason: "extra fields would be dropped",
    })
}
