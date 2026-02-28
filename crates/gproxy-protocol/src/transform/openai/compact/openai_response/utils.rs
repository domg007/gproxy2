use crate::openai::compact_response::response::{
    OpenAiCompactedResponseObject, ResponseBody as CompactResponseBody,
};
use crate::openai::compact_response::types as ct;
use crate::openai::count_tokens::types as ot;
use crate::openai::create_response::response::ResponseBody as OpenAiCreateResponseBody;
use crate::openai::create_response::types as rt;

fn compact_usage_default() -> ct::ResponseUsage {
    ct::ResponseUsage {
        input_tokens: 0,
        input_tokens_details: ct::ResponseInputTokensDetails { cached_tokens: 0 },
        output_tokens: 0,
        output_tokens_details: ct::ResponseOutputTokensDetails {
            reasoning_tokens: 0,
        },
        total_tokens: 0,
    }
}

fn compact_usage_from_create_response(usage: Option<rt::ResponseUsage>) -> ct::ResponseUsage {
    usage
        .map(|usage| ct::ResponseUsage {
            input_tokens: usage.input_tokens,
            input_tokens_details: ct::ResponseInputTokensDetails {
                cached_tokens: usage.input_tokens_details.cached_tokens,
            },
            output_tokens: usage.output_tokens,
            output_tokens_details: ct::ResponseOutputTokensDetails {
                reasoning_tokens: usage.output_tokens_details.reasoning_tokens,
            },
            total_tokens: usage.total_tokens,
        })
        .unwrap_or_else(compact_usage_default)
}

fn compact_message_content_from_response_output(
    content: ot::ResponseOutputContent,
) -> ct::CompactedResponseMessageContent {
    match content {
        ot::ResponseOutputContent::Text(text) => {
            ct::CompactedResponseMessageContent::OutputText(text)
        }
        ot::ResponseOutputContent::Refusal(refusal) => {
            ct::CompactedResponseMessageContent::Refusal(refusal)
        }
    }
}

fn compact_message_from_response_message(
    message: ot::ResponseOutputMessage,
) -> ct::CompactedResponseMessage {
    ct::CompactedResponseMessage {
        id: message.id,
        content: message
            .content
            .into_iter()
            .map(compact_message_content_from_response_output)
            .collect::<Vec<_>>(),
        role: match message.role {
            ot::ResponseOutputMessageRole::Assistant => ct::CompactedResponseMessageRole::Assistant,
        },
        status: message.status,
        type_: ct::CompactedResponseMessageType::Message,
    }
}

fn compact_fallback_message(id: String, text: String) -> ct::CompactedResponseOutputItem {
    ct::CompactedResponseOutputItem::Message(ct::CompactedResponseMessage {
        id,
        content: vec![ct::CompactedResponseMessageContent::Text(
            ct::CompactedResponseTextContent {
                text,
                type_: ct::CompactedResponseTextContentType::Text,
            },
        )],
        role: ct::CompactedResponseMessageRole::Assistant,
        status: ot::ResponseItemStatus::Completed,
        type_: ct::CompactedResponseMessageType::Message,
    })
}

fn compact_output_item_from_response_item(
    item: rt::ResponseOutputItem,
) -> Option<ct::CompactedResponseOutputItem> {
    Some(match item {
        rt::ResponseOutputItem::Message(message) => {
            ct::CompactedResponseOutputItem::Message(compact_message_from_response_message(message))
        }
        rt::ResponseOutputItem::FileSearchToolCall(item) => {
            ct::CompactedResponseOutputItem::FileSearchToolCall(item)
        }
        rt::ResponseOutputItem::ComputerToolCall(item) => {
            ct::CompactedResponseOutputItem::ComputerToolCall(item)
        }
        rt::ResponseOutputItem::ComputerCallOutput(item) => {
            ct::CompactedResponseOutputItem::ComputerCallOutput(item)
        }
        rt::ResponseOutputItem::FunctionWebSearch(item) => {
            ct::CompactedResponseOutputItem::FunctionWebSearch(item)
        }
        rt::ResponseOutputItem::FunctionToolCall(item) => {
            ct::CompactedResponseOutputItem::FunctionToolCall(item)
        }
        rt::ResponseOutputItem::FunctionCallOutput(item) => {
            ct::CompactedResponseOutputItem::FunctionCallOutput(item)
        }
        rt::ResponseOutputItem::ReasoningItem(item) => {
            ct::CompactedResponseOutputItem::ReasoningItem(item)
        }
        rt::ResponseOutputItem::CompactionItem(item) => {
            ct::CompactedResponseOutputItem::CompactionItem(item)
        }
        rt::ResponseOutputItem::ImageGenerationCall(item) => compact_fallback_message(
            item.id,
            if item.result.is_empty() {
                "[image_generation_call]".to_string()
            } else {
                item.result
            },
        ),
        rt::ResponseOutputItem::CodeInterpreterToolCall(item) => {
            ct::CompactedResponseOutputItem::CodeInterpreterToolCall(item)
        }
        rt::ResponseOutputItem::LocalShellCall(item) => {
            ct::CompactedResponseOutputItem::LocalShellCall(item)
        }
        rt::ResponseOutputItem::LocalShellCallOutput(item) => {
            ct::CompactedResponseOutputItem::LocalShellCallOutput(item)
        }
        rt::ResponseOutputItem::ShellCall(item) => ct::CompactedResponseOutputItem::ShellCall(item),
        rt::ResponseOutputItem::ShellCallOutput(item) => {
            ct::CompactedResponseOutputItem::ShellCallOutput(item)
        }
        rt::ResponseOutputItem::ApplyPatchCall(item) => {
            ct::CompactedResponseOutputItem::ApplyPatchCall(item)
        }
        rt::ResponseOutputItem::ApplyPatchCallOutput(item) => {
            ct::CompactedResponseOutputItem::ApplyPatchCallOutput(item)
        }
        rt::ResponseOutputItem::McpListTools(item) => {
            ct::CompactedResponseOutputItem::McpListTools(item)
        }
        rt::ResponseOutputItem::McpApprovalRequest(item) => {
            ct::CompactedResponseOutputItem::McpApprovalRequest(item)
        }
        rt::ResponseOutputItem::McpApprovalResponse(item) => {
            ct::CompactedResponseOutputItem::McpApprovalResponse(item)
        }
        rt::ResponseOutputItem::McpCall(item) => ct::CompactedResponseOutputItem::McpCall(item),
        rt::ResponseOutputItem::CustomToolCallOutput(item) => {
            ct::CompactedResponseOutputItem::CustomToolCallOutput(item)
        }
        rt::ResponseOutputItem::CustomToolCall(item) => {
            ct::CompactedResponseOutputItem::CustomToolCall(item)
        }
        rt::ResponseOutputItem::ItemReference(item) => {
            ct::CompactedResponseOutputItem::ItemReference(item)
        }
    })
}

pub fn compact_response_body_from_create_response(
    body: OpenAiCreateResponseBody,
) -> CompactResponseBody {
    CompactResponseBody {
        id: body.id,
        created_at: body.created_at,
        object: OpenAiCompactedResponseObject::ResponseCompaction,
        output: body
            .output
            .into_iter()
            .filter_map(compact_output_item_from_response_item)
            .collect::<Vec<_>>(),
        usage: compact_usage_from_create_response(body.usage),
    }
}
