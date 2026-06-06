//! OpenAI -> Claude compact-content transforms.

use crate::protocol::{claude, openai};
use crate::transform::{TransformContext, TransformError};

const DEFAULT_COMPACT_MAX_TOKENS: u64 = 16_384;
const DEFAULT_MODEL: &str = "unknown";

pub fn request(
    input: openai::CompactResponseRequestBody,
    _: &TransformContext,
) -> Result<claude::CreateMessageRequestBody, TransformError> {
    Ok(claude::CreateMessageRequestBody {
        model: claude::ClaudeModel::Unknown(model_to_string(&input.model)),
        messages: openai_input_to_claude_messages(input.input),
        max_tokens: DEFAULT_COMPACT_MAX_TOKENS,
        cache_control: None,
        container: None,
        context_management: Some(compact_context_management(input.instructions.as_deref())),
        diagnostics: None,
        inference_geo: None,
        mcp_servers: None,
        metadata: None,
        output_config: None,
        output_format: None,
        service_tier: compact_service_tier_to_claude(input.service_tier),
        speed: None,
        stop_sequences: None,
        stream: None,
        system: input.instructions.map(claude::SystemPrompt::String),
        temperature: None,
        thinking: None,
        tool_choice: None,
        tools: None,
        top_k: None,
        top_p: None,
        user_profile_id: None,
        extra: Default::default(),
    })
}

pub fn response(
    input: openai::CompactedResponseObject,
    _: &TransformContext,
) -> claude::CreateMessageResponseBody {
    claude::CreateMessageResponseBody {
        id: input.id,
        type_: claude::MessageObjectType::Known(claude::MessageObjectTypeKnown::Message),
        role: claude::AssistantRole::Known(claude::AssistantRoleKnown::Assistant),
        content: compact_output_to_claude_content(input.output),
        model: claude::ClaudeModel::Unknown(DEFAULT_MODEL.to_owned()),
        stop_reason: claude::StopReason::Known(claude::StopReasonKnown::Compaction),
        stop_sequence: None,
        usage: openai_usage_to_claude(input.usage),
        container: None,
        context_management: None,
        diagnostics: None,
        stop_details: None,
        extra: Default::default(),
    }
}

fn compact_context_management(instructions: Option<&str>) -> claude::ContextManagementConfig {
    claude::ContextManagementConfig {
        edits: Some(vec![claude::ContextEdit::Known(
            claude::KnownContextEdit::Compact {
                instructions: instructions.map(str::to_owned),
                pause_after_compaction: Some(true),
                trigger: None,
                extra: Default::default(),
            },
        )]),
        extra: Default::default(),
    }
}

fn openai_input_to_claude_messages(
    input: Option<openai::ResponseInput>,
) -> Vec<claude::MessageParam> {
    match input {
        Some(openai::ResponseInput::Text(text)) => text_to_claude_message(
            claude::MessageRole::Known(claude::MessageRoleKnown::User),
            text,
        )
        .into_iter()
        .collect(),
        Some(openai::ResponseInput::Items(items)) => items
            .into_iter()
            .filter_map(openai_item_to_claude_message)
            .collect(),
        None => Vec::new(),
    }
}

fn openai_item_to_claude_message(item: openai::ResponseItem) -> Option<claude::MessageParam> {
    match item {
        openai::ResponseItem::Message(message) => openai_message_to_claude_message(message),
        openai::ResponseItem::Typed(openai::TypedResponseItem::Compaction {
            encrypted_content,
            ..
        }) => Some(claude::MessageParam {
            role: claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
            content: claude::MessageContent::Array(vec![claude::ContentBlockParam::Compaction(
                claude::CompactionBlock {
                    content: None,
                    encrypted_content: Some(encrypted_content),
                    type_: claude::CompactionBlockType::Compaction,
                    cache_control: None,
                },
            )]),
            extra: Default::default(),
        }),
        _ => None,
    }
}

fn openai_message_to_claude_message(
    message: openai::ResponseMessageItem,
) -> Option<claude::MessageParam> {
    match message {
        openai::ResponseMessageItem::EasyInput(message) => {
            let role = easy_input_role_to_claude(message.role);
            let text = easy_input_content_to_text(message.content);
            text_to_claude_message(role, text)
        }
        openai::ResponseMessageItem::Input(message) => {
            let role = input_role_to_claude(message.role);
            let text = input_parts_to_text(message.content);
            text_to_claude_message(role, text)
        }
        openai::ResponseMessageItem::Output(message) => {
            let text = output_parts_to_text(message.content);
            text_to_claude_message(
                claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
                text,
            )
        }
    }
}

fn text_to_claude_message(role: claude::MessageRole, text: String) -> Option<claude::MessageParam> {
    if text.is_empty() {
        return None;
    }

    Some(claude::MessageParam {
        role,
        content: claude::MessageContent::String(text),
        extra: Default::default(),
    })
}

fn easy_input_role_to_claude(role: openai::ResponseEasyInputMessageRole) -> claude::MessageRole {
    match role {
        openai::ResponseEasyInputMessageRole::Assistant => {
            claude::MessageRole::Known(claude::MessageRoleKnown::Assistant)
        }
        openai::ResponseEasyInputMessageRole::System
        | openai::ResponseEasyInputMessageRole::Developer => {
            claude::MessageRole::Known(claude::MessageRoleKnown::System)
        }
        openai::ResponseEasyInputMessageRole::User => {
            claude::MessageRole::Known(claude::MessageRoleKnown::User)
        }
    }
}

fn input_role_to_claude(role: openai::ResponseInputMessageRole) -> claude::MessageRole {
    match role {
        openai::ResponseInputMessageRole::System | openai::ResponseInputMessageRole::Developer => {
            claude::MessageRole::Known(claude::MessageRoleKnown::System)
        }
        openai::ResponseInputMessageRole::User => {
            claude::MessageRole::Known(claude::MessageRoleKnown::User)
        }
    }
}

fn easy_input_content_to_text(content: openai::ResponseEasyInputContent) -> String {
    match content {
        openai::ResponseEasyInputContent::Text(text) => text,
        openai::ResponseEasyInputContent::Parts(parts) => input_parts_to_text(parts),
    }
}

fn input_parts_to_text(parts: Vec<openai::ResponseInputContentPart>) -> String {
    join_text(parts.into_iter().filter_map(|part| match part {
        openai::ResponseInputContentPart::InputText { text, .. } => Some(text),
        _ => None,
    }))
}

fn output_parts_to_text(parts: Vec<openai::ResponseMessageOutputContentPart>) -> String {
    join_text(parts.into_iter().map(|part| match part {
        openai::ResponseMessageOutputContentPart::OutputText { text, .. } => text,
        openai::ResponseMessageOutputContentPart::Refusal { refusal, .. } => refusal,
    }))
}

fn compact_output_to_claude_content(
    output: Vec<openai::CompactResponseItem>,
) -> Vec<claude::ContentBlock> {
    output
        .into_iter()
        .flat_map(compact_item_to_claude_content)
        .collect()
}

fn compact_item_to_claude_content(item: openai::CompactResponseItem) -> Vec<claude::ContentBlock> {
    match item {
        openai::CompactResponseItem::Message(message) => compact_message_to_claude_content(message),
        openai::CompactResponseItem::Typed(openai::TypedResponseItem::Compaction {
            encrypted_content,
            ..
        }) => vec![claude::ContentBlock::Compaction(
            claude::ResponseCompactionBlock {
                content: None,
                encrypted_content,
                type_: claude::CompactionBlockType::Compaction,
                extra: Default::default(),
            },
        )],
        _ => Vec::new(),
    }
}

fn compact_message_to_claude_content(
    message: openai::CompactMessageItem,
) -> Vec<claude::ContentBlock> {
    message
        .content
        .into_iter()
        .filter_map(compact_content_part_to_claude)
        .collect()
}

fn compact_content_part_to_claude(
    part: openai::CompactMessageContentPart,
) -> Option<claude::ContentBlock> {
    let text = match part {
        openai::CompactMessageContentPart::Input(openai::ResponseInputContentPart::InputText {
            text,
            ..
        })
        | openai::CompactMessageContentPart::Output(
            openai::ResponseOutputContentPart::OutputText { text, .. },
        )
        | openai::CompactMessageContentPart::Output(
            openai::ResponseOutputContentPart::ReasoningText { text, .. },
        )
        | openai::CompactMessageContentPart::Text(openai::CompactTextContent { text, .. })
        | openai::CompactMessageContentPart::SummaryText(openai::CompactSummaryTextContent {
            text,
            ..
        }) => text,
        openai::CompactMessageContentPart::Output(openai::ResponseOutputContentPart::Refusal {
            refusal,
            ..
        }) => refusal,
        _ => return None,
    };

    Some(claude::ContentBlock::Text(claude::ResponseTextBlock {
        citations: None,
        text,
        type_: claude::TextBlockType::Text,
        extra: Default::default(),
    }))
}

fn openai_usage_to_claude(usage: openai::ResponseUsage) -> claude::Usage {
    claude::Usage {
        input_tokens: Some(u64::from(usage.input_tokens)),
        output_tokens: Some(u64::from(usage.output_tokens)),
        cache_creation_input_tokens: None,
        cache_read_input_tokens: usage
            .input_tokens_details
            .map(|details| u64::from(details.cached_tokens)),
        cache_creation: None,
        output_tokens_details: Some(claude::OutputTokensDetails {
            thinking_tokens: u64::from(usage.output_tokens_details.reasoning_tokens),
            extra: Default::default(),
        }),
        server_tool_use: None,
        iterations: None,
        inference_geo: None,
        service_tier: None,
        speed: None,
        extra: Default::default(),
    }
}

fn compact_service_tier_to_claude(
    service_tier: Option<openai::CompactServiceTier>,
) -> Option<claude::RequestServiceTier> {
    let service_tier = match service_tier? {
        openai::CompactServiceTier::Auto => claude::RequestServiceTierKnown::Auto,
        openai::CompactServiceTier::Default => claude::RequestServiceTierKnown::StandardOnly,
        openai::CompactServiceTier::Flex | openai::CompactServiceTier::Priority => {
            claude::RequestServiceTierKnown::Auto
        }
    };
    Some(claude::RequestServiceTier::Known(service_tier))
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

fn join_text(parts: impl Iterator<Item = String>) -> String {
    parts
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
