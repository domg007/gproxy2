//! OpenAI -> Claude compact-content transforms.

use crate::protocol::{claude, openai};
use crate::transform::{TransformContext, TransformError};

const DEFAULT_COMPACT_MAX_TOKENS: u64 = 32_768;
const DEFAULT_MODEL: &str = "unknown";

pub fn request_headers(_: &TransformContext) -> claude::CreateMessageRequestHeaders {
    claude::CreateMessageRequestHeaders {
        anthropic_beta: Some(vec![claude::AnthropicBeta::Known(
            claude::AnthropicBetaKnown::ContextManagement20250627,
        )]),
        extra: Default::default(),
    }
}

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
        diagnostics: openai_previous_response_id_to_claude(input.previous_response_id),
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
        openai::ResponseItem::Typed(typed) => typed_item_to_claude_message(typed),
        _ => None,
    }
}

fn openai_message_to_claude_message(
    message: openai::ResponseMessageItem,
) -> Option<claude::MessageParam> {
    match message {
        openai::ResponseMessageItem::EasyInput(message) => {
            let role = easy_input_role_to_claude(message.role);
            let blocks = easy_input_content_to_blocks(message.content);
            blocks_to_claude_message(role, blocks)
        }
        openai::ResponseMessageItem::Input(message) => {
            let role = input_role_to_claude(message.role);
            let blocks = input_parts_to_blocks(message.content);
            blocks_to_claude_message(role, blocks)
        }
        openai::ResponseMessageItem::Output(message) => {
            let blocks = output_parts_to_blocks(message.content);
            blocks_to_claude_message(
                claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
                blocks,
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

fn blocks_to_claude_message(
    role: claude::MessageRole,
    blocks: Vec<claude::ContentBlockParam>,
) -> Option<claude::MessageParam> {
    if blocks.is_empty() {
        return None;
    }

    Some(claude::MessageParam {
        role,
        content: claude::MessageContent::Array(blocks),
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

fn easy_input_content_to_blocks(
    content: openai::ResponseEasyInputContent,
) -> Vec<claude::ContentBlockParam> {
    match content {
        openai::ResponseEasyInputContent::Text(text) => text_block(text).into_iter().collect(),
        openai::ResponseEasyInputContent::Parts(parts) => input_parts_to_blocks(parts),
    }
}

fn input_parts_to_blocks(
    parts: Vec<openai::ResponseInputContentPart>,
) -> Vec<claude::ContentBlockParam> {
    parts
        .into_iter()
        .filter_map(input_part_to_claude_block)
        .collect()
}

fn input_part_to_claude_block(
    part: openai::ResponseInputContentPart,
) -> Option<claude::ContentBlockParam> {
    match part {
        openai::ResponseInputContentPart::InputText { text, .. } => text_block(text),
        openai::ResponseInputContentPart::InputImage {
            file_id, image_url, ..
        } => image_block(file_id, image_url),
        openai::ResponseInputContentPart::InputFile {
            file_data,
            file_id,
            file_url,
            filename,
            ..
        } => document_block(file_id, file_url, file_data, filename),
        openai::ResponseInputContentPart::InputAudio { .. } => None,
    }
}

fn output_parts_to_blocks(
    parts: Vec<openai::ResponseMessageOutputContentPart>,
) -> Vec<claude::ContentBlockParam> {
    parts
        .into_iter()
        .filter_map(|part| match part {
            openai::ResponseMessageOutputContentPart::OutputText { text, .. } => text_block(text),
            openai::ResponseMessageOutputContentPart::Refusal { refusal, .. } => {
                text_block(refusal)
            }
        })
        .collect()
}

fn typed_item_to_claude_message(item: openai::TypedResponseItem) -> Option<claude::MessageParam> {
    let (role, blocks) = match item {
        openai::TypedResponseItem::FunctionCall {
            arguments,
            call_id,
            name,
            id,
            ..
        } => (
            claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
            vec![claude::ContentBlockParam::ToolUse(claude::ToolUseBlock {
                id: id.unwrap_or(call_id),
                input: arguments_to_json_object(&arguments),
                name,
                type_: claude::ToolUseBlockType::ToolUse,
                cache_control: None,
                caller: None,
            })],
        ),
        openai::TypedResponseItem::CustomToolCall {
            call_id,
            input,
            name,
            id,
            ..
        } => (
            claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
            vec![claude::ContentBlockParam::ToolUse(claude::ToolUseBlock {
                id: id.unwrap_or(call_id),
                input: string_input_json_object(input),
                name,
                type_: claude::ToolUseBlockType::ToolUse,
                cache_control: None,
                caller: None,
            })],
        ),
        openai::TypedResponseItem::WebSearchCall { id, action, .. } => (
            claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
            vec![claude::ContentBlockParam::ServerToolUse(
                server_tool_use_block(
                    id,
                    serializable_to_json_object(&action),
                    claude::ServerToolUseNameKnown::WebSearch,
                ),
            )],
        ),
        openai::TypedResponseItem::CodeInterpreterCall {
            id,
            code,
            container_id,
            ..
        } => (
            claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
            vec![claude::ContentBlockParam::ServerToolUse(
                server_tool_use_block(
                    id,
                    code_interpreter_input(code, container_id),
                    claude::ServerToolUseNameKnown::CodeExecution,
                ),
            )],
        ),
        openai::TypedResponseItem::LocalShellCall {
            action, call_id, ..
        } => (
            claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
            vec![claude::ContentBlockParam::ServerToolUse(
                server_tool_use_block(
                    call_id,
                    serializable_to_json_object(&action),
                    claude::ServerToolUseNameKnown::BashCodeExecution,
                ),
            )],
        ),
        openai::TypedResponseItem::ShellCall {
            action,
            call_id,
            environment: None,
            ..
        } => (
            claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
            vec![claude::ContentBlockParam::ServerToolUse(
                server_tool_use_block(
                    call_id,
                    serializable_to_json_object(&action),
                    claude::ServerToolUseNameKnown::BashCodeExecution,
                ),
            )],
        ),
        openai::TypedResponseItem::ShellCall {
            action,
            call_id,
            environment: Some(environment),
            ..
        } => (
            claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
            vec![claude::ContentBlockParam::ServerToolUse(
                server_tool_use_block(
                    call_id,
                    shell_input(action, environment),
                    claude::ServerToolUseNameKnown::BashCodeExecution,
                ),
            )],
        ),
        openai::TypedResponseItem::FunctionCallOutput {
            call_id, output, ..
        }
        | openai::TypedResponseItem::CustomToolCallOutput {
            call_id, output, ..
        } => (
            claude::MessageRole::Known(claude::MessageRoleKnown::User),
            vec![claude::ContentBlockParam::ToolResult(
                claude::ToolResultBlock {
                    tool_use_id: call_id,
                    type_: claude::ToolResultBlockType::ToolResult,
                    cache_control: None,
                    content: response_output_to_tool_result(output),
                    is_error: None,
                },
            )],
        ),
        openai::TypedResponseItem::McpCall {
            id,
            arguments,
            name,
            server_label,
            output,
            error,
            ..
        } => {
            let mut blocks = vec![claude::ContentBlockParam::McpToolUse(
                claude::McpToolUseBlock {
                    id: id.clone(),
                    input: arguments_to_json_object(&arguments),
                    name,
                    server_name: server_label,
                    type_: claude::McpToolUseBlockType::McpToolUse,
                    cache_control: None,
                },
            )];
            if let Some(result) = mcp_result_block(id, output, error) {
                blocks.push(claude::ContentBlockParam::McpToolResult(result));
            }
            (
                claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
                blocks,
            )
        }
        openai::TypedResponseItem::Reasoning {
            id: _,
            summary,
            content,
            encrypted_content,
            ..
        } => {
            let mut blocks = Vec::new();
            if let Some(encrypted_content) = encrypted_content {
                blocks.push(claude::ContentBlockParam::RedactedThinking(
                    claude::RedactedThinkingBlock {
                        data: encrypted_content,
                        type_: claude::RedactedThinkingBlockType::RedactedThinking,
                    },
                ));
            }
            blocks.extend(summary.into_iter().filter_map(|part| text_block(part.text)));
            blocks.extend(
                content
                    .into_iter()
                    .flatten()
                    .filter_map(|part| text_block(part.text)),
            );
            (
                claude::MessageRole::Known(claude::MessageRoleKnown::Assistant),
                blocks,
            )
        }
        _ => return None,
    };

    blocks_to_claude_message(role, blocks)
}

fn text_block(text: String) -> Option<claude::ContentBlockParam> {
    if text.is_empty() {
        return None;
    }

    Some(claude::ContentBlockParam::Text(claude::TextBlock {
        text,
        type_: claude::TextBlockType::Text,
        cache_control: None,
        citations: None,
        extra: Default::default(),
    }))
}

fn image_block(
    file_id: Option<String>,
    image_url: Option<String>,
) -> Option<claude::ContentBlockParam> {
    let source = if let Some(file_id) = file_id {
        claude::ImageSource::File(claude::FileImageSource {
            file_id,
            type_: claude::FileSourceType::File,
            extra: Default::default(),
        })
    } else {
        claude::ImageSource::Url(claude::UrlImageSource {
            type_: claude::UrlSourceType::Url,
            url: image_url?,
            extra: Default::default(),
        })
    };

    Some(claude::ContentBlockParam::Image(claude::ImageBlock {
        source,
        type_: claude::ImageBlockType::Image,
        cache_control: None,
    }))
}

fn document_block(
    file_id: Option<String>,
    file_url: Option<String>,
    file_data: Option<String>,
    filename: Option<String>,
) -> Option<claude::ContentBlockParam> {
    let source = if let Some(file_id) = file_id {
        claude::DocumentSource::File(claude::FileDocumentSource {
            file_id,
            type_: claude::FileSourceType::File,
            extra: Default::default(),
        })
    } else if let Some(file_url) = file_url {
        claude::DocumentSource::Url(claude::UrlDocumentSource {
            type_: claude::UrlSourceType::Url,
            url: file_url,
            extra: Default::default(),
        })
    } else {
        claude::DocumentSource::Text(claude::PlainTextSource {
            data: file_data?,
            media_type: claude::PlainTextMediaType::TextPlain,
            type_: claude::TextSourceType::Text,
            extra: Default::default(),
        })
    };

    Some(claude::ContentBlockParam::Document(claude::DocumentBlock {
        source,
        type_: claude::DocumentBlockType::Document,
        cache_control: None,
        citations: None,
        context: None,
        title: filename,
    }))
}

fn response_output_to_tool_result(
    output: openai::ResponseOutput,
) -> Option<claude::ToolResultContent> {
    match output {
        openai::ResponseOutput::Text(text) => {
            (!text.is_empty()).then_some(claude::ToolResultContent::Text(text))
        }
        openai::ResponseOutput::Parts(parts) => {
            let blocks = parts
                .into_iter()
                .filter_map(tool_output_part_to_claude)
                .collect::<Vec<_>>();
            (!blocks.is_empty()).then_some(claude::ToolResultContent::Blocks(blocks))
        }
    }
}

fn tool_output_part_to_claude(
    part: openai::ResponseToolOutputContentPart,
) -> Option<claude::ToolResultContentBlock> {
    match part {
        openai::ResponseToolOutputContentPart::InputText { text, .. } => {
            Some(claude::ToolResultContentBlock::Text(claude::TextBlock {
                text,
                type_: claude::TextBlockType::Text,
                cache_control: None,
                citations: None,
                extra: Default::default(),
            }))
        }
        openai::ResponseToolOutputContentPart::InputImage {
            file_id, image_url, ..
        } => image_block(file_id, image_url).and_then(|block| match block {
            claude::ContentBlockParam::Image(block) => {
                Some(claude::ToolResultContentBlock::Image(block))
            }
            _ => None,
        }),
        openai::ResponseToolOutputContentPart::InputFile {
            file_data,
            file_id,
            file_url,
            filename,
            ..
        } => document_block(file_id, file_url, file_data, filename).and_then(|block| match block {
            claude::ContentBlockParam::Document(block) => {
                Some(claude::ToolResultContentBlock::Document(block))
            }
            _ => None,
        }),
    }
}

fn server_tool_use_block(
    id: String,
    input: claude::JsonObject,
    name: claude::ServerToolUseNameKnown,
) -> claude::ServerToolUseBlock {
    claude::ServerToolUseBlock {
        id,
        input,
        name: claude::ServerToolUseName::Known(name),
        type_: claude::ServerToolUseBlockType::ServerToolUse,
        cache_control: None,
        caller: None,
    }
}

fn response_server_tool_use_block(
    id: String,
    input: claude::JsonObject,
    name: claude::ServerToolUseNameKnown,
) -> claude::ResponseServerToolUseBlock {
    claude::ResponseServerToolUseBlock {
        id,
        input,
        name: claude::ServerToolUseName::Known(name),
        type_: claude::ServerToolUseBlockType::ServerToolUse,
        caller: None,
        extra: Default::default(),
    }
}

fn mcp_result_block(
    tool_use_id: String,
    output: Option<String>,
    error: Option<String>,
) -> Option<claude::McpToolResultBlock> {
    let is_error = error.is_some();
    let content = error.or(output)?;
    Some(claude::McpToolResultBlock {
        tool_use_id,
        type_: claude::McpToolResultBlockType::McpToolResult,
        cache_control: None,
        content: Some(claude::McpToolResultContent::String(content)),
        is_error: Some(is_error),
    })
}

fn arguments_to_json_object(arguments: &str) -> claude::JsonObject {
    serde_json::from_str(arguments)
        .map(value_to_json_object)
        .unwrap_or_else(|_| string_input_json_object(arguments.to_owned()))
}

fn string_input_json_object(input: String) -> claude::JsonObject {
    let mut object = claude::JsonObject::new();
    object.insert("input".to_owned(), serde_json::Value::String(input));
    object
}

fn value_to_json_object(value: serde_json::Value) -> claude::JsonObject {
    match value {
        serde_json::Value::Object(map) => map.into_iter().collect(),
        value => {
            let mut object = claude::JsonObject::new();
            object.insert("value".to_owned(), value);
            object
        }
    }
}

fn serializable_to_json_object<T: serde::Serialize>(value: &T) -> claude::JsonObject {
    serde_json::to_value(value)
        .map(value_to_json_object)
        .unwrap_or_default()
}

fn code_interpreter_input(code: Option<String>, container_id: String) -> claude::JsonObject {
    let mut input = claude::JsonObject::new();
    if let Some(code) = code {
        input.insert("code".to_owned(), serde_json::Value::String(code));
    }
    input.insert(
        "container_id".to_owned(),
        serde_json::Value::String(container_id),
    );
    input
}

fn shell_input(
    action: openai::ShellAction,
    environment: openai::ShellEnvironment,
) -> claude::JsonObject {
    let mut input = serializable_to_json_object(&action);
    input.insert(
        "environment".to_owned(),
        serde_json::to_value(environment).unwrap_or(serde_json::Value::Null),
    );
    input
}

fn openai_previous_response_id_to_claude(
    previous_response_id: Option<String>,
) -> Option<claude::DiagnosticsParam> {
    Some(claude::DiagnosticsParam {
        previous_message_id: Some(Some(previous_response_id?)),
        extra: Default::default(),
    })
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
        openai::CompactResponseItem::Typed(openai::TypedResponseItem::FunctionCall {
            arguments,
            call_id,
            name,
            id,
            ..
        }) => vec![claude::ContentBlock::ToolUse(
            claude::ResponseToolUseBlock {
                id: id.unwrap_or(call_id),
                input: arguments_to_json_object(&arguments),
                name,
                type_: claude::ToolUseBlockType::ToolUse,
                caller: None,
                extra: Default::default(),
            },
        )],
        openai::CompactResponseItem::Typed(openai::TypedResponseItem::CustomToolCall {
            call_id,
            input,
            name,
            id,
            ..
        }) => vec![claude::ContentBlock::ToolUse(
            claude::ResponseToolUseBlock {
                id: id.unwrap_or(call_id),
                input: string_input_json_object(input),
                name,
                type_: claude::ToolUseBlockType::ToolUse,
                caller: None,
                extra: Default::default(),
            },
        )],
        openai::CompactResponseItem::Typed(openai::TypedResponseItem::WebSearchCall {
            id,
            action,
            ..
        }) => vec![claude::ContentBlock::ServerToolUse(
            response_server_tool_use_block(
                id,
                serializable_to_json_object(&action),
                claude::ServerToolUseNameKnown::WebSearch,
            ),
        )],
        openai::CompactResponseItem::Typed(openai::TypedResponseItem::CodeInterpreterCall {
            id,
            code,
            container_id,
            ..
        }) => vec![claude::ContentBlock::ServerToolUse(
            response_server_tool_use_block(
                id,
                code_interpreter_input(code, container_id),
                claude::ServerToolUseNameKnown::CodeExecution,
            ),
        )],
        openai::CompactResponseItem::Typed(openai::TypedResponseItem::LocalShellCall {
            action,
            call_id,
            ..
        }) => vec![claude::ContentBlock::ServerToolUse(
            response_server_tool_use_block(
                call_id,
                serializable_to_json_object(&action),
                claude::ServerToolUseNameKnown::BashCodeExecution,
            ),
        )],
        openai::CompactResponseItem::Typed(openai::TypedResponseItem::ShellCall {
            action,
            call_id,
            environment: None,
            ..
        }) => vec![claude::ContentBlock::ServerToolUse(
            response_server_tool_use_block(
                call_id,
                serializable_to_json_object(&action),
                claude::ServerToolUseNameKnown::BashCodeExecution,
            ),
        )],
        openai::CompactResponseItem::Typed(openai::TypedResponseItem::ShellCall {
            action,
            call_id,
            environment: Some(environment),
            ..
        }) => vec![claude::ContentBlock::ServerToolUse(
            response_server_tool_use_block(
                call_id,
                shell_input(action, environment),
                claude::ServerToolUseNameKnown::BashCodeExecution,
            ),
        )],
        openai::CompactResponseItem::Typed(openai::TypedResponseItem::McpCall {
            id,
            arguments,
            name,
            server_label,
            output,
            error,
            ..
        }) => {
            let mut blocks = vec![claude::ContentBlock::McpToolUse(
                claude::ResponseMcpToolUseBlock {
                    id: id.clone(),
                    input: arguments_to_json_object(&arguments),
                    name,
                    server_name: server_label,
                    type_: claude::ResponseMcpToolUseBlockType::McpToolUse,
                    extra: Default::default(),
                },
            )];
            if let Some(result) = response_mcp_result_block(id, output, error) {
                blocks.push(claude::ContentBlock::McpToolResult(result));
            }
            blocks
        }
        openai::CompactResponseItem::Typed(openai::TypedResponseItem::Reasoning {
            id,
            summary,
            content,
            encrypted_content,
            ..
        }) => reasoning_to_claude_content(id, summary, content, encrypted_content),
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

fn response_mcp_result_block(
    tool_use_id: String,
    output: Option<String>,
    error: Option<String>,
) -> Option<claude::ResponseMcpToolResultBlock> {
    let is_error = error.is_some();
    let content = error.or(output)?;
    Some(claude::ResponseMcpToolResultBlock {
        content: claude::ResponseMcpToolResultContent::String(content),
        is_error,
        tool_use_id,
        type_: claude::ResponseMcpToolResultBlockType::McpToolResult,
        extra: Default::default(),
    })
}

fn reasoning_to_claude_content(
    id: String,
    summary: Vec<openai::ResponseReasoningSummaryPart>,
    content: Option<Vec<openai::ResponseReasoningTextPart>>,
    encrypted_content: Option<String>,
) -> Vec<claude::ContentBlock> {
    let mut blocks = Vec::new();
    if let Some(encrypted_content) = encrypted_content {
        blocks.push(claude::ContentBlock::RedactedThinking(
            claude::RedactedThinkingBlock {
                data: encrypted_content,
                type_: claude::RedactedThinkingBlockType::RedactedThinking,
            },
        ));
    }

    let thinking = join_text(content.into_iter().flatten().map(|part| part.text));
    if !thinking.is_empty() {
        blocks.push(claude::ContentBlock::Thinking(claude::ThinkingBlock {
            signature: id,
            thinking,
            type_: claude::ThinkingBlockType::Thinking,
        }));
    }

    blocks.extend(summary.into_iter().map(|part| {
        claude::ContentBlock::Text(claude::ResponseTextBlock {
            citations: None,
            text: part.text,
            type_: claude::TextBlockType::Text,
            extra: Default::default(),
        })
    }));
    blocks
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
