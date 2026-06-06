//! Claude -> OpenAI compact-content transforms.

use crate::protocol::{claude, openai};
use crate::transform::{TransformContext, TransformError};

const DEFAULT_MODEL: &str = "unknown";
const DEFAULT_REASONING_ID: &str = "reasoning";

pub fn request(
    input: claude::CreateMessageRequestBody,
    _: &TransformContext,
) -> Result<openai::CompactResponseRequestBody, TransformError> {
    let compact_instructions = compact_instructions(input.context_management.as_ref());
    let system = input.system.and_then(claude_system_to_text);
    let mut input_items = claude_messages_to_openai_items(input.messages);
    if compact_instructions.is_some()
        && let Some(system) = system.as_ref()
    {
        input_items.insert(0, system_to_openai_item(system.clone()));
    }

    Ok(openai::CompactResponseRequestBody {
        input: Some(openai::ResponseInput::Items(input_items)),
        instructions: compact_instructions.or(system),
        model: openai::OpenAiModelId::Unknown(model_to_string(&input.model)),
        previous_response_id: claude_previous_message_id_to_openai(input.diagnostics),
        prompt_cache_key: None,
        prompt_cache_retention: None,
        service_tier: claude_service_tier_to_compact(input.service_tier),
        extra: Default::default(),
    })
}

fn system_to_openai_item(text: String) -> openai::ResponseItem {
    openai::ResponseItem::Message(openai::ResponseMessageItem::EasyInput(
        openai::ResponseEasyInputMessageItem {
            type_: Some(openai::ResponseMessageItemType::Message),
            role: openai::ResponseEasyInputMessageRole::System,
            content: openai::ResponseEasyInputContent::Text(text),
            phase: None,
            extra: Default::default(),
        },
    ))
}

pub fn response(
    input: claude::CreateMessageResponseBody,
    _: &TransformContext,
) -> openai::CompactedResponseObject {
    openai::CompactedResponseObject {
        id: input.id.clone(),
        created_at: 0,
        object: openai::ResponseCompactionObjectType::ResponseCompaction,
        output: claude_content_to_compact_output(input.id, input.content),
        usage: claude_usage_to_openai(input.usage),
        extra: Default::default(),
    }
}

fn compact_instructions(
    context_management: Option<&claude::ContextManagementConfig>,
) -> Option<String> {
    context_management
        .and_then(|context| context.edits.as_ref())
        .and_then(|edits| {
            edits.iter().find_map(|edit| match edit {
                claude::ContextEdit::Known(claude::KnownContextEdit::Compact {
                    instructions,
                    ..
                }) => instructions.clone(),
                _ => None,
            })
        })
}

fn claude_messages_to_openai_items(
    messages: Vec<claude::MessageParam>,
) -> Vec<openai::ResponseItem> {
    messages
        .into_iter()
        .flat_map(claude_message_to_openai_items)
        .collect()
}

fn claude_message_to_openai_items(message: claude::MessageParam) -> Vec<openai::ResponseItem> {
    let role = claude_role_to_openai(message.role);
    let mut items = Vec::new();
    let mut message_parts = Vec::new();

    match message.content {
        claude::MessageContent::String(text) => {
            if !text.is_empty() {
                message_parts.push(openai::ResponseInputContentPart::InputText {
                    text,
                    extra: Default::default(),
                });
            }
        }
        claude::MessageContent::Array(blocks) => {
            for block in blocks {
                match claude_request_block_to_openai(block) {
                    ClaudeRequestBlockItem::MessagePart(part) => message_parts.push(part),
                    ClaudeRequestBlockItem::Typed(item) => items.push(item),
                    ClaudeRequestBlockItem::None => {}
                }
            }
        }
    }

    if !message_parts.is_empty() {
        items.push(openai::ResponseItem::Message(
            openai::ResponseMessageItem::EasyInput(openai::ResponseEasyInputMessageItem {
                type_: Some(openai::ResponseMessageItemType::Message),
                role,
                content: openai::ResponseEasyInputContent::Parts(message_parts),
                phase: None,
                extra: Default::default(),
            }),
        ));
    }

    items
}

enum ClaudeRequestBlockItem {
    MessagePart(openai::ResponseInputContentPart),
    Typed(openai::ResponseItem),
    None,
}

fn claude_role_to_openai(role: claude::MessageRole) -> openai::ResponseEasyInputMessageRole {
    match role {
        claude::MessageRole::Known(claude::MessageRoleKnown::Assistant) => {
            openai::ResponseEasyInputMessageRole::Assistant
        }
        claude::MessageRole::Known(claude::MessageRoleKnown::System) => {
            openai::ResponseEasyInputMessageRole::System
        }
        claude::MessageRole::Known(claude::MessageRoleKnown::User)
        | claude::MessageRole::Unknown(_) => openai::ResponseEasyInputMessageRole::User,
    }
}

fn claude_request_block_to_openai(block: claude::ContentBlockParam) -> ClaudeRequestBlockItem {
    match block {
        claude::ContentBlockParam::Text(block) => {
            ClaudeRequestBlockItem::MessagePart(openai::ResponseInputContentPart::InputText {
                text: block.text,
                extra: Default::default(),
            })
        }
        claude::ContentBlockParam::Image(block) => image_source_to_input_part(block.source)
            .map(ClaudeRequestBlockItem::MessagePart)
            .unwrap_or(ClaudeRequestBlockItem::None),
        claude::ContentBlockParam::Document(block) => {
            document_source_to_input_part(block.source, block.title)
                .map(ClaudeRequestBlockItem::MessagePart)
                .unwrap_or(ClaudeRequestBlockItem::None)
        }
        claude::ContentBlockParam::ToolUse(block) => ClaudeRequestBlockItem::Typed(
            openai::ResponseItem::Typed(openai::TypedResponseItem::FunctionCall {
                arguments: json_object_to_string(&block.input),
                call_id: block.id.clone(),
                name: block.name,
                id: Some(block.id),
                namespace: None,
                status: Some(openai::ResponseItemLifecycleStatus::Completed),
                extra: Default::default(),
            }),
        ),
        claude::ContentBlockParam::ToolResult(block) => ClaudeRequestBlockItem::Typed(
            openai::ResponseItem::Typed(openai::TypedResponseItem::FunctionCallOutput {
                call_id: block.tool_use_id,
                output: tool_result_content_to_openai(block.content),
                id: None,
                status: Some(openai::ResponseItemLifecycleStatus::Completed),
                created_by: None,
                extra: Default::default(),
            }),
        ),
        claude::ContentBlockParam::Thinking(block) => ClaudeRequestBlockItem::Typed(
            openai::ResponseItem::Typed(openai::TypedResponseItem::Reasoning {
                id: block.signature,
                summary: Vec::new(),
                content: Some(vec![openai::ResponseReasoningTextPart {
                    text: block.thinking,
                    type_: openai::ResponseReasoningTextType::ReasoningText,
                    extra: Default::default(),
                }]),
                encrypted_content: None,
                status: Some(openai::ResponseItemLifecycleStatus::Completed),
                extra: Default::default(),
            }),
        ),
        claude::ContentBlockParam::RedactedThinking(block) => ClaudeRequestBlockItem::Typed(
            openai::ResponseItem::Typed(openai::TypedResponseItem::Reasoning {
                id: DEFAULT_REASONING_ID.to_owned(),
                summary: Vec::new(),
                content: None,
                encrypted_content: Some(block.data),
                status: Some(openai::ResponseItemLifecycleStatus::Completed),
                extra: Default::default(),
            }),
        ),
        claude::ContentBlockParam::Compaction(block) => {
            let Some(encrypted_content) = block.encrypted_content else {
                return block
                    .content
                    .map(|text| {
                        ClaudeRequestBlockItem::MessagePart(
                            openai::ResponseInputContentPart::InputText {
                                text,
                                extra: Default::default(),
                            },
                        )
                    })
                    .unwrap_or(ClaudeRequestBlockItem::None);
            };
            ClaudeRequestBlockItem::Typed(openai::ResponseItem::Typed(
                openai::TypedResponseItem::Compaction {
                    encrypted_content,
                    id: None,
                    created_by: None,
                    extra: Default::default(),
                },
            ))
        }
        claude::ContentBlockParam::McpToolUse(block) => ClaudeRequestBlockItem::Typed(
            openai::ResponseItem::Typed(openai::TypedResponseItem::McpCall {
                id: block.id,
                arguments: json_object_to_string(&block.input),
                name: block.name,
                server_label: block.server_name,
                approval_request_id: None,
                error: None,
                output: None,
                status: Some(openai::ResponseMcpCallStatus::Completed),
                extra: Default::default(),
            }),
        ),
        claude::ContentBlockParam::MidConversationSystem(block) => {
            let text = join_text(block.content.into_iter().map(|block| block.text));
            if text.is_empty() {
                ClaudeRequestBlockItem::None
            } else {
                ClaudeRequestBlockItem::MessagePart(openai::ResponseInputContentPart::InputText {
                    text,
                    extra: Default::default(),
                })
            }
        }
        _ => ClaudeRequestBlockItem::None,
    }
}

fn claude_content_to_compact_output(
    id: String,
    content: Vec<claude::ContentBlock>,
) -> Vec<openai::CompactResponseItem> {
    let mut message_parts = Vec::new();
    let mut output = Vec::new();

    for block in content {
        match block {
            claude::ContentBlock::Text(block) => {
                message_parts.push(openai::CompactMessageContentPart::Text(
                    openai::CompactTextContent {
                        text: block.text,
                        type_: openai::CompactTextContentType::Text,
                        extra: Default::default(),
                    },
                ));
            }
            claude::ContentBlock::Thinking(block) => {
                output.push(openai::CompactResponseItem::Typed(
                    openai::TypedResponseItem::Reasoning {
                        id: block.signature,
                        summary: Vec::new(),
                        content: Some(vec![openai::ResponseReasoningTextPart {
                            text: block.thinking,
                            type_: openai::ResponseReasoningTextType::ReasoningText,
                            extra: Default::default(),
                        }]),
                        encrypted_content: None,
                        status: Some(openai::ResponseItemLifecycleStatus::Completed),
                        extra: Default::default(),
                    },
                ));
            }
            claude::ContentBlock::RedactedThinking(block) => {
                output.push(openai::CompactResponseItem::Typed(
                    openai::TypedResponseItem::Reasoning {
                        id: DEFAULT_REASONING_ID.to_owned(),
                        summary: Vec::new(),
                        content: None,
                        encrypted_content: Some(block.data),
                        status: Some(openai::ResponseItemLifecycleStatus::Completed),
                        extra: Default::default(),
                    },
                ));
            }
            claude::ContentBlock::ToolUse(block) => {
                output.push(openai::CompactResponseItem::Typed(
                    openai::TypedResponseItem::FunctionCall {
                        arguments: json_object_to_string(&block.input),
                        call_id: block.id.clone(),
                        name: block.name,
                        id: Some(block.id),
                        namespace: None,
                        status: Some(openai::ResponseItemLifecycleStatus::Completed),
                        extra: Default::default(),
                    },
                ));
            }
            claude::ContentBlock::McpToolUse(block) => {
                output.push(openai::CompactResponseItem::Typed(
                    openai::TypedResponseItem::McpCall {
                        id: block.id,
                        arguments: json_object_to_string(&block.input),
                        name: block.name,
                        server_label: block.server_name,
                        approval_request_id: None,
                        error: None,
                        output: None,
                        status: Some(openai::ResponseMcpCallStatus::Completed),
                        extra: Default::default(),
                    },
                ));
            }
            claude::ContentBlock::Compaction(block) => {
                if let Some(text) = block.content {
                    message_parts.push(openai::CompactMessageContentPart::SummaryText(
                        openai::CompactSummaryTextContent {
                            text,
                            type_: openai::CompactSummaryTextContentType::SummaryText,
                            extra: Default::default(),
                        },
                    ));
                }
                output.push(openai::CompactResponseItem::Typed(
                    openai::TypedResponseItem::Compaction {
                        encrypted_content: block.encrypted_content,
                        id: None,
                        created_by: None,
                        extra: Default::default(),
                    },
                ));
            }
            _ => {}
        }
    }

    if !message_parts.is_empty() {
        output.insert(
            0,
            openai::CompactResponseItem::Message(openai::CompactMessageItem {
                id,
                type_: openai::ResponseMessageItemType::Message,
                content: message_parts,
                role: openai::CompactMessageRole::Assistant,
                status: openai::ResponseItemLifecycleStatus::Completed,
                phase: None,
                extra: Default::default(),
            }),
        );
    }

    output
}

fn claude_previous_message_id_to_openai(
    diagnostics: Option<claude::DiagnosticsParam>,
) -> Option<String> {
    diagnostics?.previous_message_id?
}

fn image_source_to_input_part(
    source: claude::ImageSource,
) -> Option<openai::ResponseInputContentPart> {
    match source {
        claude::ImageSource::File(source) => Some(openai::ResponseInputContentPart::InputImage {
            detail: None,
            file_id: Some(source.file_id),
            image_url: None,
            extra: Default::default(),
        }),
        claude::ImageSource::Url(source) => Some(openai::ResponseInputContentPart::InputImage {
            detail: None,
            file_id: None,
            image_url: Some(source.url),
            extra: Default::default(),
        }),
        claude::ImageSource::Base64(source) => Some(openai::ResponseInputContentPart::InputImage {
            detail: None,
            file_id: None,
            image_url: Some(format!(
                "data:{};base64,{}",
                image_media_type(source.media_type),
                source.data
            )),
            extra: Default::default(),
        }),
        claude::ImageSource::Raw(_) => None,
    }
}

fn document_source_to_input_part(
    source: claude::DocumentSource,
    filename: Option<String>,
) -> Option<openai::ResponseInputContentPart> {
    match source {
        claude::DocumentSource::File(source) => Some(openai::ResponseInputContentPart::InputFile {
            detail: None,
            file_data: None,
            file_id: Some(source.file_id),
            file_url: None,
            filename,
            extra: Default::default(),
        }),
        claude::DocumentSource::Url(source) => Some(openai::ResponseInputContentPart::InputFile {
            detail: None,
            file_data: None,
            file_id: None,
            file_url: Some(source.url),
            filename,
            extra: Default::default(),
        }),
        claude::DocumentSource::Text(source) => Some(openai::ResponseInputContentPart::InputFile {
            detail: None,
            file_data: Some(source.data),
            file_id: None,
            file_url: None,
            filename,
            extra: Default::default(),
        }),
        claude::DocumentSource::Base64(source) => {
            Some(openai::ResponseInputContentPart::InputFile {
                detail: None,
                file_data: Some(format!(
                    "data:{};base64,{}",
                    pdf_media_type(source.media_type),
                    source.data
                )),
                file_id: None,
                file_url: None,
                filename,
                extra: Default::default(),
            })
        }
        claude::DocumentSource::Content(source) => {
            content_source_text(source.content).map(|file_data| {
                openai::ResponseInputContentPart::InputFile {
                    detail: None,
                    file_data: Some(file_data),
                    file_id: None,
                    file_url: None,
                    filename,
                    extra: Default::default(),
                }
            })
        }
        claude::DocumentSource::Raw(_) => None,
    }
}

fn json_object_to_string(object: &claude::JsonObject) -> String {
    serde_json::to_string(object).unwrap_or_else(|_| "{}".to_owned())
}

fn tool_result_content_to_openai(
    content: Option<claude::ToolResultContent>,
) -> openai::ResponseOutput {
    match content {
        Some(claude::ToolResultContent::Text(text)) => openai::ResponseOutput::Text(text),
        Some(claude::ToolResultContent::Blocks(blocks)) => {
            let parts = blocks
                .into_iter()
                .filter_map(tool_result_block_to_openai)
                .collect::<Vec<_>>();
            openai::ResponseOutput::Parts(parts)
        }
        Some(claude::ToolResultContent::Raw(value)) => {
            openai::ResponseOutput::Text(value.to_string())
        }
        None => openai::ResponseOutput::Text(String::new()),
    }
}

fn tool_result_block_to_openai(
    block: claude::ToolResultContentBlock,
) -> Option<openai::ResponseToolOutputContentPart> {
    match block {
        claude::ToolResultContentBlock::Text(block) => {
            Some(openai::ResponseToolOutputContentPart::InputText {
                text: block.text,
                extra: Default::default(),
            })
        }
        claude::ToolResultContentBlock::Image(block) => {
            input_part_to_tool_output_part(image_source_to_input_part(block.source)?)
        }
        claude::ToolResultContentBlock::Document(block) => input_part_to_tool_output_part(
            document_source_to_input_part(block.source, block.title)?,
        ),
        claude::ToolResultContentBlock::SearchResult(block) => {
            let text = join_text(
                block
                    .content
                    .into_iter()
                    .map(|content_block| content_block.text)
                    .chain([block.source, block.title]),
            );
            (!text.is_empty()).then_some(openai::ResponseToolOutputContentPart::InputText {
                text,
                extra: Default::default(),
            })
        }
        claude::ToolResultContentBlock::ToolReference(_)
        | claude::ToolResultContentBlock::Raw(_) => None,
    }
}

fn input_part_to_tool_output_part(
    part: openai::ResponseInputContentPart,
) -> Option<openai::ResponseToolOutputContentPart> {
    match part {
        openai::ResponseInputContentPart::InputText { text, .. } => {
            Some(openai::ResponseToolOutputContentPart::InputText {
                text,
                extra: Default::default(),
            })
        }
        openai::ResponseInputContentPart::InputImage {
            detail,
            file_id,
            image_url,
            ..
        } => Some(openai::ResponseToolOutputContentPart::InputImage {
            detail,
            file_id,
            image_url,
            extra: Default::default(),
        }),
        openai::ResponseInputContentPart::InputFile {
            detail,
            file_data,
            file_id,
            file_url,
            filename,
            ..
        } => Some(openai::ResponseToolOutputContentPart::InputFile {
            detail,
            file_data,
            file_id,
            file_url,
            filename,
            extra: Default::default(),
        }),
        openai::ResponseInputContentPart::InputAudio { .. } => None,
    }
}

fn content_source_text(content: claude::ContentSourceContent) -> Option<String> {
    let text = match content {
        claude::ContentSourceContent::Text(text) => text,
        claude::ContentSourceContent::Blocks(blocks) => {
            join_text(blocks.into_iter().filter_map(|block| match block {
                claude::ContentSourceBlock::Text(block) => Some(block.text),
                claude::ContentSourceBlock::Image(_) | claude::ContentSourceBlock::Raw(_) => None,
            }))
        }
    };
    (!text.is_empty()).then_some(text)
}

fn image_media_type(media_type: claude::ImageMediaType) -> &'static str {
    match media_type {
        claude::ImageMediaType::Jpeg => "image/jpeg",
        claude::ImageMediaType::Png => "image/png",
        claude::ImageMediaType::Gif => "image/gif",
        claude::ImageMediaType::Webp => "image/webp",
    }
}

fn pdf_media_type(media_type: claude::PdfMediaType) -> &'static str {
    match media_type {
        claude::PdfMediaType::ApplicationPdf => "application/pdf",
    }
}

fn claude_usage_to_openai(usage: claude::Usage) -> openai::ResponseUsage {
    let input_tokens = u64_to_u32(usage.input_tokens.unwrap_or_default());
    let output_tokens = u64_to_u32(usage.output_tokens.unwrap_or_default());
    let cached_tokens = usage.cache_read_input_tokens.map(u64_to_u32);
    let reasoning_tokens = usage
        .output_tokens_details
        .map(|details| u64_to_u32(details.thinking_tokens))
        .unwrap_or_default();

    openai::ResponseUsage {
        input_tokens,
        output_tokens,
        total_tokens: input_tokens.saturating_add(output_tokens),
        input_tokens_details: cached_tokens.map(|cached_tokens| {
            openai::ResponseInputTokensDetails {
                cached_tokens,
                extra: Default::default(),
            }
        }),
        output_tokens_details: openai::ResponseOutputTokensDetails {
            reasoning_tokens,
            extra: Default::default(),
        },
        extra: Default::default(),
    }
}

fn claude_system_to_text(system: claude::SystemPrompt) -> Option<String> {
    match system {
        claude::SystemPrompt::String(text) => Some(text).filter(|text| !text.is_empty()),
        claude::SystemPrompt::Array(blocks) => {
            let text = join_text(blocks.into_iter().map(|block| block.text));
            (!text.is_empty()).then_some(text)
        }
    }
}

fn claude_service_tier_to_compact(
    service_tier: Option<claude::RequestServiceTier>,
) -> Option<openai::CompactServiceTier> {
    let service_tier = match service_tier? {
        claude::RequestServiceTier::Known(claude::RequestServiceTierKnown::Auto) => {
            openai::CompactServiceTier::Auto
        }
        claude::RequestServiceTier::Known(claude::RequestServiceTierKnown::StandardOnly)
        | claude::RequestServiceTier::Unknown(_) => openai::CompactServiceTier::Default,
    };
    Some(service_tier)
}

fn model_to_string<T: serde::Serialize>(model: &T) -> String {
    let Ok(value) = serde_json::to_value(model) else {
        return DEFAULT_MODEL.to_owned();
    };
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| DEFAULT_MODEL.to_owned())
}

fn u64_to_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn join_text(parts: impl Iterator<Item = String>) -> String {
    parts
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
