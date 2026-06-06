//! Claude -> OpenAI compact-content transforms.

use crate::protocol::{claude, openai};
use crate::transform::{TransformContext, TransformError};

const DEFAULT_MODEL: &str = "unknown";

pub fn request(
    input: claude::CreateMessageRequestBody,
    _: &TransformContext,
) -> Result<openai::CompactResponseRequestBody, TransformError> {
    let compact_instructions = compact_instructions(input.context_management.as_ref());
    let system = input.system.and_then(claude_system_to_text);

    Ok(openai::CompactResponseRequestBody {
        input: Some(openai::ResponseInput::Items(
            claude_messages_to_openai_items(input.messages),
        )),
        instructions: compact_instructions.or(system),
        model: openai::OpenAiModelId::Unknown(model_to_string(&input.model)),
        previous_response_id: None,
        prompt_cache_key: None,
        prompt_cache_retention: None,
        service_tier: claude_service_tier_to_compact(input.service_tier),
        extra: Default::default(),
    })
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
    let (text, compactions) = claude_message_content_to_openai(message.content);
    let mut items = Vec::new();

    if !text.is_empty() {
        items.push(openai::ResponseItem::Message(
            openai::ResponseMessageItem::EasyInput(openai::ResponseEasyInputMessageItem {
                type_: Some(openai::ResponseMessageItemType::Message),
                role,
                content: openai::ResponseEasyInputContent::Text(text),
                phase: None,
                extra: Default::default(),
            }),
        ));
    }

    items.extend(compactions.into_iter().map(|encrypted_content| {
        openai::ResponseItem::Typed(openai::TypedResponseItem::Compaction {
            encrypted_content,
            id: None,
            created_by: None,
            extra: Default::default(),
        })
    }));

    items
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

fn claude_message_content_to_openai(content: claude::MessageContent) -> (String, Vec<String>) {
    match content {
        claude::MessageContent::String(text) => (text, Vec::new()),
        claude::MessageContent::Array(blocks) => {
            let mut text_parts = Vec::new();
            let mut compactions = Vec::new();

            for block in blocks {
                match block {
                    claude::ContentBlockParam::Text(block) => text_parts.push(block.text),
                    claude::ContentBlockParam::Compaction(block) => {
                        if let Some(content) = block.content {
                            text_parts.push(content);
                        }
                        if let Some(encrypted_content) = block.encrypted_content {
                            compactions.push(encrypted_content);
                        }
                    }
                    _ => {}
                }
            }

            (join_text(text_parts.into_iter()), compactions)
        }
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
