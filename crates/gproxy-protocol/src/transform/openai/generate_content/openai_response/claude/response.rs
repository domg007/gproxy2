use std::collections::BTreeMap;

use crate::claude::count_tokens::types as ct;
use crate::claude::create_message::response::ClaudeCreateMessageResponse;
use crate::claude::create_message::types::{BetaServiceTier, BetaStopReason};
use crate::openai::count_tokens::types as ot;
use crate::openai::create_response::response::{OpenAiCreateResponseResponse, ResponseBody};
use crate::openai::create_response::types as rt;
use crate::openai::types::OpenAiResponseHeaders;
use crate::transform::claude::utils::claude_model_to_string;
use crate::transform::openai::generate_content::openai_chat_completions::claude::utils::server_tool_name;
use crate::transform::openai::model_list::claude::utils::openai_error_response_from_claude;
use crate::transform::utils::TransformError;

impl TryFrom<ClaudeCreateMessageResponse> for OpenAiCreateResponseResponse {
    type Error = TransformError;

    fn try_from(value: ClaudeCreateMessageResponse) -> Result<Self, TransformError> {
        Ok(match value {
            ClaudeCreateMessageResponse::Success {
                stats_code,
                headers,
                body,
            } => {
                let mut output = Vec::new();
                let mut message_content = Vec::new();
                let mut output_text_parts = Vec::new();
                let mut tool_call_count = 0usize;

                for (index, block) in body.content.into_iter().enumerate() {
                    match block {
                        crate::claude::create_message::types::BetaContentBlock::Text(block) => {
                            if !block.text.is_empty() {
                                output_text_parts.push(block.text.clone());
                                message_content.push(ot::ResponseOutputContent::Text(
                                    ot::ResponseOutputText {
                                        annotations: Vec::new(),
                                        logprobs: None,
                                        text: block.text,
                                        type_: ot::ResponseOutputTextType::OutputText,
                                    },
                                ));
                            }
                        }
                        crate::claude::create_message::types::BetaContentBlock::Thinking(block) => {
                            output.push(rt::ResponseOutputItem::ReasoningItem(
                                ot::ResponseReasoningItem {
                                    id: Some(format!("reasoning_{index}")),
                                    summary: vec![ot::ResponseSummaryTextContent {
                                        text: block.thinking.clone(),
                                        type_: ot::ResponseSummaryTextContentType::SummaryText,
                                    }],
                                    type_: ot::ResponseReasoningItemType::Reasoning,
                                    content: Some(vec![ot::ResponseReasoningTextContent {
                                        text: block.thinking,
                                        type_: ot::ResponseReasoningTextContentType::ReasoningText,
                                    }]),
                                    encrypted_content: None,
                                    status: Some(ot::ResponseItemStatus::Completed),
                                },
                            ));
                        }
                        crate::claude::create_message::types::BetaContentBlock::RedactedThinking(
                            block,
                        ) => {
                            output.push(rt::ResponseOutputItem::ReasoningItem(
                                ot::ResponseReasoningItem {
                                    id: Some(format!("redacted_reasoning_{index}")),
                                    summary: Vec::new(),
                                    type_: ot::ResponseReasoningItemType::Reasoning,
                                    content: None,
                                    encrypted_content: Some(block.data),
                                    status: Some(ot::ResponseItemStatus::Completed),
                                },
                            ));
                        }
                        crate::claude::create_message::types::BetaContentBlock::ToolUse(block) => {
                            tool_call_count += 1;
                            output.push(rt::ResponseOutputItem::FunctionToolCall(
                                ot::ResponseFunctionToolCall {
                                    arguments: serde_json::to_string(&block.input)
                                        .unwrap_or_else(|_| "{}".to_string()),
                                    call_id: block.id.clone(),
                                    name: block.name,
                                    type_: ot::ResponseFunctionToolCallType::FunctionCall,
                                    id: Some(block.id),
                                    status: Some(ot::ResponseItemStatus::Completed),
                                },
                            ));
                        }
                        crate::claude::create_message::types::BetaContentBlock::ServerToolUse(
                            block,
                        ) => {
                            tool_call_count += 1;
                            match block.name {
                                ct::BetaServerToolUseName::CodeExecution => {
                                    let code = block
                                        .input
                                        .get("code")
                                        .and_then(|value| value.as_str())
                                        .unwrap_or_default()
                                        .to_string();
                                    let container_id = block
                                        .input
                                        .get("container_id")
                                        .and_then(|value| value.as_str())
                                        .unwrap_or_default()
                                        .to_string();
                                    output.push(rt::ResponseOutputItem::CodeInterpreterToolCall(
                                        ot::ResponseCodeInterpreterToolCall {
                                            id: block.id,
                                            code,
                                            container_id,
                                            outputs: None,
                                            status: ot::ResponseCodeInterpreterToolCallStatus::Completed,
                                            type_: ot::ResponseCodeInterpreterToolCallType::CodeInterpreterCall,
                                        },
                                    ));
                                }
                                ct::BetaServerToolUseName::WebSearch
                                | ct::BetaServerToolUseName::WebFetch => {
                                    let query = block
                                        .input
                                        .get("query")
                                        .and_then(|value| value.as_str())
                                        .map(ToString::to_string);
                                    output.push(rt::ResponseOutputItem::FunctionWebSearch(
                                        ot::ResponseFunctionWebSearch {
                                            id: block.id,
                                            action: ot::ResponseFunctionWebSearchAction::Search {
                                                query,
                                                queries: None,
                                                sources: None,
                                            },
                                            status: ot::ResponseFunctionWebSearchStatus::Completed,
                                            type_: ot::ResponseFunctionWebSearchType::WebSearchCall,
                                        },
                                    ));
                                }
                                ct::BetaServerToolUseName::BashCodeExecution => {
                                    let mut commands = block
                                        .input
                                        .get("commands")
                                        .and_then(|value| value.as_array())
                                        .map(|values| {
                                            values
                                                .iter()
                                                .filter_map(|value| {
                                                    value.as_str().map(ToString::to_string)
                                                })
                                                .collect::<Vec<_>>()
                                        })
                                        .unwrap_or_default();
                                    if commands.is_empty()
                                        && let Some(command) = block
                                            .input
                                            .get("command")
                                            .and_then(|value| value.as_str())
                                        {
                                            commands.push(command.to_string());
                                        }
                                    output.push(rt::ResponseOutputItem::ShellCall(
                                        ot::ResponseShellCall {
                                            action: ot::ResponseShellCallAction {
                                                commands,
                                                max_output_length: None,
                                                timeout_ms: None,
                                            },
                                            call_id: block.id.clone(),
                                            type_: ot::ResponseShellCallType::ShellCall,
                                            id: Some(block.id),
                                            environment: None,
                                            status: Some(ot::ResponseItemStatus::Completed),
                                        },
                                    ));
                                }
                                _ => {
                                    output.push(rt::ResponseOutputItem::CustomToolCall(
                                        ot::ResponseCustomToolCall {
                                            call_id: block.id.clone(),
                                            input: serde_json::to_string(&block.input)
                                                .unwrap_or_else(|_| "{}".to_string()),
                                            name: server_tool_name(&block.name),
                                            type_: ot::ResponseCustomToolCallType::CustomToolCall,
                                            id: Some(block.id),
                                        },
                                    ));
                                }
                            }
                        }
                        crate::claude::create_message::types::BetaContentBlock::McpToolUse(block) => {
                            tool_call_count += 1;
                            output.push(rt::ResponseOutputItem::McpCall(ot::ResponseMcpCall {
                                id: block.id,
                                arguments: serde_json::to_string(&block.input)
                                    .unwrap_or_else(|_| "{}".to_string()),
                                name: block.name,
                                server_label: block.server_name,
                                type_: ot::ResponseMcpCallType::McpCall,
                                approval_request_id: None,
                                error: None,
                                output: None,
                                status: Some(ot::ResponseToolCallStatus::Completed),
                            }));
                        }
                        crate::claude::create_message::types::BetaContentBlock::McpToolResult(block) => {
                            let output_text = match block.content {
                                Some(ct::BetaMcpToolResultBlockParamContent::Text(text)) => text,
                                Some(ct::BetaMcpToolResultBlockParamContent::Blocks(parts)) => parts
                                    .into_iter()
                                    .map(|part| part.text)
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                                None => String::new(),
                            };
                            output.push(rt::ResponseOutputItem::FunctionCallOutput(
                                ot::ResponseFunctionCallOutput {
                                    call_id: block.tool_use_id,
                                    output: ot::ResponseFunctionCallOutputContent::Text(output_text),
                                    type_: ot::ResponseFunctionCallOutputType::FunctionCallOutput,
                                    id: None,
                                    status: Some(ot::ResponseItemStatus::Completed),
                                },
                            ));
                        }
                        crate::claude::create_message::types::BetaContentBlock::Compaction(block) => {
                            output.push(rt::ResponseOutputItem::CompactionItem(
                                ot::ResponseCompactionItemParam {
                                    encrypted_content: block.content.unwrap_or_default(),
                                    type_: ot::ResponseCompactionItemType::Compaction,
                                    id: None,
                                },
                            ));
                        }
                        crate::claude::create_message::types::BetaContentBlock::ContainerUpload(block) => {
                            message_content.push(ot::ResponseOutputContent::Text(
                                ot::ResponseOutputText {
                                    annotations: Vec::new(),
                                    logprobs: None,
                                    text: format!("container_upload:{}", block.file_id),
                                    type_: ot::ResponseOutputTextType::OutputText,
                                },
                            ));
                        }
                        crate::claude::create_message::types::BetaContentBlock::WebSearchToolResult(
                            block,
                        ) => {
                            let text = match block.content {
                                crate::claude::create_message::types::BetaWebSearchToolResultBlockContent::Results(results) => results
                                    .into_iter()
                                    .map(|item| format!("{}\n{}", item.title, item.url))
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                                crate::claude::create_message::types::BetaWebSearchToolResultBlockContent::Error(err) => {
                                    format!("web_search_error:{:?}", err.error_code)
                                }
                            };
                            if !text.is_empty() {
                                output.push(rt::ResponseOutputItem::FunctionCallOutput(
                                    ot::ResponseFunctionCallOutput {
                                        call_id: block.tool_use_id,
                                        output: ot::ResponseFunctionCallOutputContent::Text(text),
                                        type_: ot::ResponseFunctionCallOutputType::FunctionCallOutput,
                                        id: None,
                                        status: Some(ot::ResponseItemStatus::Completed),
                                    },
                                ));
                            }
                        }
                        crate::claude::create_message::types::BetaContentBlock::WebFetchToolResult(
                            block,
                        ) => {
                            let text = match block.content {
                                crate::claude::create_message::types::BetaWebFetchToolResultBlockContent::Result(result) => {
                                    result.url
                                }
                                crate::claude::create_message::types::BetaWebFetchToolResultBlockContent::Error(err) => {
                                    format!("web_fetch_error:{:?}", err.error_code)
                                }
                            };
                            if !text.is_empty() {
                                output.push(rt::ResponseOutputItem::FunctionCallOutput(
                                    ot::ResponseFunctionCallOutput {
                                        call_id: block.tool_use_id,
                                        output: ot::ResponseFunctionCallOutputContent::Text(text),
                                        type_: ot::ResponseFunctionCallOutputType::FunctionCallOutput,
                                        id: None,
                                        status: Some(ot::ResponseItemStatus::Completed),
                                    },
                                ));
                            }
                        }
                        crate::claude::create_message::types::BetaContentBlock::CodeExecutionToolResult(block) => {
                            let text = match block.content {
                                ct::BetaCodeExecutionToolResultBlockParamContent::Result(result) => {
                                    if result.stderr.is_empty() {
                                        result.stdout
                                    } else if result.stdout.is_empty() {
                                        result.stderr
                                    } else {
                                        format!("stdout: {}\nstderr: {}", result.stdout, result.stderr)
                                    }
                                }
                                ct::BetaCodeExecutionToolResultBlockParamContent::Error(err) => {
                                    format!("code_execution_error:{:?}", err.error_code)
                                }
                            };
                            output.push(rt::ResponseOutputItem::FunctionCallOutput(
                                ot::ResponseFunctionCallOutput {
                                    call_id: block.tool_use_id,
                                    output: ot::ResponseFunctionCallOutputContent::Text(text),
                                    type_: ot::ResponseFunctionCallOutputType::FunctionCallOutput,
                                    id: None,
                                    status: Some(ot::ResponseItemStatus::Completed),
                                },
                            ));
                        }
                        crate::claude::create_message::types::BetaContentBlock::BashCodeExecutionToolResult(block) => {
                            let text = match block.content {
                                ct::BetaBashCodeExecutionToolResultBlockParamContent::Result(result) => {
                                    if result.stderr.is_empty() {
                                        result.stdout
                                    } else if result.stdout.is_empty() {
                                        result.stderr
                                    } else {
                                        format!("stdout: {}\nstderr: {}", result.stdout, result.stderr)
                                    }
                                }
                                ct::BetaBashCodeExecutionToolResultBlockParamContent::Error(err) => {
                                    format!("bash_code_execution_error:{:?}", err.error_code)
                                }
                            };
                            output.push(rt::ResponseOutputItem::FunctionCallOutput(
                                ot::ResponseFunctionCallOutput {
                                    call_id: block.tool_use_id,
                                    output: ot::ResponseFunctionCallOutputContent::Text(text),
                                    type_: ot::ResponseFunctionCallOutputType::FunctionCallOutput,
                                    id: None,
                                    status: Some(ot::ResponseItemStatus::Completed),
                                },
                            ));
                        }
                        crate::claude::create_message::types::BetaContentBlock::TextEditorCodeExecutionToolResult(block) => {
                            let text = match block.content {
                                ct::BetaTextEditorCodeExecutionToolResultBlockParamContent::View(view) => {
                                    view.content
                                }
                                ct::BetaTextEditorCodeExecutionToolResultBlockParamContent::Create(create) => {
                                    format!("file_updated:{}", create.is_file_update)
                                }
                                ct::BetaTextEditorCodeExecutionToolResultBlockParamContent::StrReplace(replace) => {
                                    replace.lines.unwrap_or_default().join("\n")
                                }
                                ct::BetaTextEditorCodeExecutionToolResultBlockParamContent::Error(err) => err
                                    .error_message
                                    .unwrap_or_else(|| {
                                        format!("text_editor_code_execution_error:{:?}", err.error_code)
                                    }),
                            };
                            output.push(rt::ResponseOutputItem::FunctionCallOutput(
                                ot::ResponseFunctionCallOutput {
                                    call_id: block.tool_use_id,
                                    output: ot::ResponseFunctionCallOutputContent::Text(text),
                                    type_: ot::ResponseFunctionCallOutputType::FunctionCallOutput,
                                    id: None,
                                    status: Some(ot::ResponseItemStatus::Completed),
                                },
                            ));
                        }
                        crate::claude::create_message::types::BetaContentBlock::ToolSearchToolResult(block) => {
                            let text = match block.content {
                                ct::BetaToolSearchToolResultBlockParamContent::Result(result) => result
                                    .tool_references
                                    .into_iter()
                                    .map(|reference| reference.tool_name)
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                                ct::BetaToolSearchToolResultBlockParamContent::Error(err) => {
                                    format!("tool_search_error:{:?}", err.error_code)
                                }
                            };
                            output.push(rt::ResponseOutputItem::FunctionCallOutput(
                                ot::ResponseFunctionCallOutput {
                                    call_id: block.tool_use_id,
                                    output: ot::ResponseFunctionCallOutputContent::Text(text),
                                    type_: ot::ResponseFunctionCallOutputType::FunctionCallOutput,
                                    id: None,
                                    status: Some(ot::ResponseItemStatus::Completed),
                                },
                            ));
                        }
                    }
                }

                if !message_content.is_empty() {
                    output.insert(
                        0,
                        rt::ResponseOutputItem::Message(ot::ResponseOutputMessage {
                            id: format!("{}_message_0", body.id),
                            content: message_content,
                            role: ot::ResponseOutputMessageRole::Assistant,
                            phase: Some(ot::ResponseMessagePhase::FinalAnswer),
                            status: ot::ResponseItemStatus::Completed,
                            type_: ot::ResponseOutputMessageType::Message,
                        }),
                    );
                }

                let (status, incomplete_details) = match body.stop_reason {
                    Some(BetaStopReason::MaxTokens)
                    | Some(BetaStopReason::ModelContextWindowExceeded) => (
                        Some(rt::ResponseStatus::Incomplete),
                        Some(rt::ResponseIncompleteDetails {
                            reason: Some(rt::ResponseIncompleteReason::MaxOutputTokens),
                        }),
                    ),
                    Some(BetaStopReason::Refusal) => (
                        Some(rt::ResponseStatus::Incomplete),
                        Some(rt::ResponseIncompleteDetails {
                            reason: Some(rt::ResponseIncompleteReason::ContentFilter),
                        }),
                    ),
                    _ => (Some(rt::ResponseStatus::Completed), None),
                };
                let service_tier = Some(match body.usage.service_tier.clone() {
                    BetaServiceTier::Standard => rt::ResponseServiceTier::Default,
                    BetaServiceTier::Priority => rt::ResponseServiceTier::Priority,
                    BetaServiceTier::Batch => rt::ResponseServiceTier::Flex,
                });
                let input_tokens = body
                    .usage
                    .input_tokens
                    .saturating_add(body.usage.cache_creation_input_tokens)
                    .saturating_add(body.usage.cache_read_input_tokens);
                let usage = Some(rt::ResponseUsage {
                    input_tokens,
                    input_tokens_details: rt::ResponseInputTokensDetails {
                        cached_tokens: body.usage.cache_read_input_tokens,
                    },
                    output_tokens: body.usage.output_tokens,
                    output_tokens_details: rt::ResponseOutputTokensDetails {
                        reasoning_tokens: 0,
                    },
                    total_tokens: input_tokens.saturating_add(body.usage.output_tokens),
                });

                OpenAiCreateResponseResponse::Success {
                    stats_code,
                    headers: OpenAiResponseHeaders {
                        extra: headers.extra,
                    },
                    body: ResponseBody {
                        id: body.id,
                        created_at: 0,
                        error: None,
                        incomplete_details,
                        instructions: Some(ot::ResponseInput::Text(String::new())),
                        metadata: BTreeMap::new(),
                        model: claude_model_to_string(&body.model),
                        object: rt::ResponseObject::Response,
                        output,
                        parallel_tool_calls: tool_call_count > 1,
                        temperature: 1.0,
                        tool_choice: if tool_call_count > 0 {
                            ot::ResponseToolChoice::Options(ot::ResponseToolChoiceOptions::Required)
                        } else {
                            ot::ResponseToolChoice::Options(ot::ResponseToolChoiceOptions::Auto)
                        },
                        tools: Vec::new(),
                        top_p: 1.0,
                        background: None,
                        completed_at: None,
                        conversation: None,
                        max_output_tokens: None,
                        max_tool_calls: None,
                        output_text: if output_text_parts.is_empty() {
                            None
                        } else {
                            Some(output_text_parts.join("\n"))
                        },
                        previous_response_id: None,
                        prompt: None,
                        prompt_cache_key: None,
                        prompt_cache_retention: None,
                        reasoning: None,
                        safety_identifier: None,
                        service_tier,
                        status,
                        text: None,
                        top_logprobs: None,
                        truncation: None,
                        usage,
                        user: None,
                    },
                }
            }
            ClaudeCreateMessageResponse::Error {
                stats_code,
                headers,
                body,
            } => OpenAiCreateResponseResponse::Error {
                stats_code,
                headers: OpenAiResponseHeaders {
                    extra: headers.extra,
                },
                body: openai_error_response_from_claude(stats_code, body),
            },
        })
    }
}
