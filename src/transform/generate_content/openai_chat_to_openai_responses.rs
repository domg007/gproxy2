//! OpenAI Chat Completions -> OpenAI Responses transforms.

use crate::protocol::openai::*;
use crate::transform::{TransformContext, TransformError};

pub fn request(
    input: ChatCompletionRequest,
    _: &TransformContext,
) -> Result<ResponseCreateRequest, TransformError> {
    reject_some(&input.audio, "audio")?;
    reject_some(&input.frequency_penalty, "frequency_penalty")?;
    reject_some(&input.function_call, "function_call")?;
    reject_some(&input.functions, "functions")?;
    reject_some(&input.logit_bias, "logit_bias")?;
    reject_some(&input.logprobs, "logprobs")?;
    reject_some(&input.modalities, "modalities")?;
    reject_some(&input.n, "n")?;
    reject_some(&input.prediction, "prediction")?;
    reject_some(&input.presence_penalty, "presence_penalty")?;
    reject_some(&input.seed, "seed")?;
    reject_some(&input.stop, "stop")?;
    reject_some(&input.tool_choice, "tool_choice")?;
    reject_some(&input.tools, "tools")?;
    reject_some(&input.web_search_options, "web_search_options")?;

    let max_output_tokens = merge_max_tokens(input.max_completion_tokens, input.max_tokens)?;
    let stream_options = input
        .stream_options
        .map(|options| {
            reject_some(&options.include_usage, "stream_options.include_usage")?;
            Ok(ResponseStreamOptions {
                include_obfuscation: options.include_obfuscation,
                extra: Extra::new(),
            })
        })
        .transpose()?;

    let text = match (input.response_format, input.verbosity) {
        (None, None) => None,
        (format, verbosity) => Some(TextConfig {
            format: format
                .map(chat_response_format_to_response_format)
                .transpose()?,
            verbosity,
            extra: Extra::new(),
        }),
    };

    Ok(ResponseCreateRequest {
        background: None,
        context_management: None,
        conversation: None,
        include: None,
        input: Some(ResponseInput::Items(chat_messages_to_response_items(
            input.messages,
        )?)),
        instructions: None,
        max_output_tokens,
        max_tool_calls: None,
        metadata: input.metadata,
        model: Some(input.model),
        moderation: input.moderation,
        parallel_tool_calls: input.parallel_tool_calls,
        previous_response_id: None,
        prompt_cache_key: input.prompt_cache_key,
        prompt_cache_retention: input.prompt_cache_retention,
        prompt: None,
        reasoning: input.reasoning_effort.map(|effort| ReasoningConfig {
            effort: Some(effort),
            summary: None,
            generate_summary: None,
            extra: Extra::new(),
        }),
        safety_identifier: input.safety_identifier,
        service_tier: input.service_tier,
        store: input.store,
        stream: input.stream,
        stream_options,
        temperature: input.temperature,
        text,
        tool_choice: None,
        tools: None,
        top_logprobs: input.top_logprobs,
        top_p: input.top_p,
        truncation: None,
        user: input.user,
        extra: Extra::new(),
    })
}

fn chat_messages_to_response_items(
    messages: Vec<ChatCompletionMessageParam>,
) -> Result<Vec<ResponseItem>, TransformError> {
    messages
        .into_iter()
        .map(chat_message_to_response_item)
        .collect()
}

fn chat_message_to_response_item(
    message: ChatCompletionMessageParam,
) -> Result<ResponseItem, TransformError> {
    match message {
        ChatCompletionMessageParam::Developer { content, .. } => Ok(easy_input(
            ResponseEasyInputMessageRole::Developer,
            chat_text_to_easy_content(content)?,
        )),
        ChatCompletionMessageParam::System { content, .. } => Ok(easy_input(
            ResponseEasyInputMessageRole::System,
            chat_text_to_easy_content(content)?,
        )),
        ChatCompletionMessageParam::User { content, .. } => Ok(easy_input(
            ResponseEasyInputMessageRole::User,
            chat_content_to_easy_content(content)?,
        )),
        ChatCompletionMessageParam::Assistant {
            content,
            audio,
            function_call,
            refusal,
            tool_calls,
            ..
        } => {
            reject_some(&audio, "message.audio")?;
            reject_some(&function_call, "message.function_call")?;
            reject_some(&refusal, "message.refusal")?;
            reject_some(&tool_calls, "message.tool_calls")?;
            let content = content
                .map(chat_assistant_content_to_easy_content)
                .transpose()?
                .unwrap_or_else(|| ResponseEasyInputContent::Text(String::new()));
            Ok(easy_input(ResponseEasyInputMessageRole::Assistant, content))
        }
        ChatCompletionMessageParam::Tool { .. } => Err(TransformError::UnsupportedField {
            field: "message",
            reason: "tool messages need pair-specific tool result mapping",
        }),
        ChatCompletionMessageParam::Function { .. } => Err(TransformError::UnsupportedField {
            field: "message",
            reason: "legacy function messages need pair-specific mapping",
        }),
    }
}

fn easy_input(
    role: ResponseEasyInputMessageRole,
    content: ResponseEasyInputContent,
) -> ResponseItem {
    ResponseItem::Message(ResponseMessageItem::EasyInput(
        ResponseEasyInputMessageItem {
            type_: Some(ResponseMessageItemType::Message),
            role,
            content,
            phase: None,
            extra: Extra::new(),
        },
    ))
}

fn chat_text_to_easy_content(
    content: ChatTextContent,
) -> Result<ResponseEasyInputContent, TransformError> {
    match content {
        ChatTextContent::Text(text) => Ok(ResponseEasyInputContent::Text(text)),
        ChatTextContent::Parts(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    ChatTextContentPart::Text { text: value, .. } => {
                        text.push_str(&value);
                    }
                }
            }
            Ok(ResponseEasyInputContent::Text(text))
        }
    }
}

fn chat_assistant_content_to_easy_content(
    content: ChatAssistantContent,
) -> Result<ResponseEasyInputContent, TransformError> {
    match content {
        ChatAssistantContent::Text(text) => Ok(ResponseEasyInputContent::Text(text)),
        ChatAssistantContent::Parts(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    ChatAssistantContentPart::Text { text: value, .. } => {
                        text.push_str(&value);
                    }
                    ChatAssistantContentPart::Refusal { .. } => {
                        return Err(TransformError::UnsupportedField {
                            field: "assistant.content",
                            reason: "assistant refusal parts need response output content mapping",
                        });
                    }
                }
            }
            Ok(ResponseEasyInputContent::Text(text))
        }
    }
}

fn chat_content_to_easy_content(
    content: ChatContent,
) -> Result<ResponseEasyInputContent, TransformError> {
    match content {
        ChatContent::Text(text) => Ok(ResponseEasyInputContent::Text(text)),
        ChatContent::Parts(parts) => parts
            .into_iter()
            .map(chat_content_part_to_response_part)
            .collect::<Result<Vec<_>, _>>()
            .map(ResponseEasyInputContent::Parts),
    }
}

fn chat_content_part_to_response_part(
    part: ChatContentPart,
) -> Result<ResponseInputContentPart, TransformError> {
    match part {
        ChatContentPart::Text { text, .. } => Ok(ResponseInputContentPart::InputText {
            text,
            extra: Extra::new(),
        }),
        ChatContentPart::ImageUrl { image_url, .. } => Ok(ResponseInputContentPart::InputImage {
            detail: image_url.detail.map(chat_detail_to_response_detail),
            file_id: None,
            image_url: Some(image_url.url),
            extra: Extra::new(),
        }),
        ChatContentPart::InputAudio { input_audio, .. } => {
            Ok(ResponseInputContentPart::InputAudio {
                input_audio: InputAudioContent {
                    data: input_audio.data,
                    format: input_audio.format,
                    extra: Extra::new(),
                },
                extra: Extra::new(),
            })
        }
        ChatContentPart::File { file, .. } => Ok(ResponseInputContentPart::InputFile {
            detail: None,
            file_data: file.file_data,
            file_id: file.file_id,
            file_url: None,
            filename: file.filename,
            extra: Extra::new(),
        }),
    }
}

fn chat_response_format_to_response_format(
    format: ChatResponseFormat,
) -> Result<ResponseFormat, TransformError> {
    match format {
        ChatResponseFormat::Text(text) => Ok(ResponseFormat::Text(text)),
        ChatResponseFormat::JsonObject(json) => Ok(ResponseFormat::JsonObject(json)),
        ChatResponseFormat::ChatJsonSchema(schema) => {
            let Some(json_schema) = schema.json_schema.schema else {
                return Err(TransformError::InvalidInput {
                    reason: "chat json_schema response format requires `json_schema.schema`"
                        .to_owned(),
                });
            };
            Ok(ResponseFormat::JsonSchema(JsonSchemaResponseFormat {
                type_: schema.type_,
                name: schema.json_schema.name,
                schema: json_schema,
                description: schema.json_schema.description,
                strict: schema.json_schema.strict,
                extra: Extra::new(),
            }))
        }
    }
}

fn chat_detail_to_response_detail(detail: ChatImageDetailLevel) -> DetailLevel {
    match detail {
        ChatImageDetailLevel::Auto => DetailLevel::Auto,
        ChatImageDetailLevel::Low => DetailLevel::Low,
        ChatImageDetailLevel::High => DetailLevel::High,
    }
}

fn merge_max_tokens(
    max_completion_tokens: Option<u32>,
    max_tokens: Option<u32>,
) -> Result<Option<u32>, TransformError> {
    match (max_completion_tokens, max_tokens) {
        (Some(current), Some(legacy)) if current != legacy => Err(TransformError::InvalidInput {
            reason: "max_completion_tokens and max_tokens disagree".to_owned(),
        }),
        (Some(value), _) | (_, Some(value)) => Ok(Some(value)),
        (None, None) => Ok(None),
    }
}

fn reject_some<T>(value: &Option<T>, field: &'static str) -> Result<(), TransformError> {
    if value.is_some() {
        return Err(TransformError::UnsupportedField {
            field,
            reason: "field has no safe OpenAI Responses equivalent in this transform",
        });
    }
    Ok(())
}
