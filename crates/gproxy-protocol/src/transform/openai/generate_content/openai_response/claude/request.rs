use crate::claude::count_tokens::types as ct;
use crate::claude::create_message::request::{
    ClaudeCreateMessageRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::claude::create_message::types::{
    BetaMetadata, BetaServiceTierParam, BetaSpeed, HttpMethod as ClaudeHttpMethod, Model,
};
use crate::openai::count_tokens::types as ot;
use crate::openai::create_response::request::OpenAiCreateResponseRequest;
use crate::openai::create_response::types::{ResponseContextManagementType, ResponseServiceTier};
use crate::transform::openai::count_tokens::claude::utils::{
    mcp_allowed_tools_to_configs, openai_mcp_tool_to_server, openai_message_content_to_claude,
    openai_reasoning_to_claude, openai_role_to_claude, openai_tool_choice_to_claude,
    parallel_disable, tool_from_function,
};
use crate::transform::openai::count_tokens::openai::utils::{
    openai_function_call_output_content_to_text, openai_input_to_items,
    openai_reasoning_summary_to_text,
};
use crate::transform::utils::TransformError;

impl TryFrom<OpenAiCreateResponseRequest> for ClaudeCreateMessageRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseRequest) -> Result<Self, TransformError> {
        let body = value.body;
        let mut messages = Vec::new();

        for item in openai_input_to_items(body.input.clone()) {
            match item {
                ot::ResponseInputItem::Message(message) => {
                    messages.push(ct::BetaMessageParam {
                        content: openai_message_content_to_claude(message.content),
                        role: openai_role_to_claude(message.role),
                    });
                }
                ot::ResponseInputItem::OutputMessage(message) => {
                    let text = message
                        .content
                        .into_iter()
                        .map(|part| match part {
                            ot::ResponseOutputContent::Text(text) => text.text,
                            ot::ResponseOutputContent::Refusal(refusal) => refusal.refusal,
                        })
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !text.is_empty() {
                        messages.push(ct::BetaMessageParam {
                            content: ct::BetaMessageContent::Text(text),
                            role: ct::BetaMessageRole::Assistant,
                        });
                    }
                }
                ot::ResponseInputItem::FunctionToolCall(tool_call) => {
                    let input = serde_json::from_str::<ct::JsonObject>(&tool_call.arguments)
                        .unwrap_or_default();
                    messages.push(ct::BetaMessageParam {
                        content: ct::BetaMessageContent::Blocks(vec![
                            ct::BetaContentBlockParam::ToolUse(ct::BetaToolUseBlockParam {
                                id: tool_call.call_id,
                                input,
                                name: tool_call.name,
                                type_: ct::BetaToolUseBlockType::ToolUse,
                                cache_control: None,
                                caller: None,
                            }),
                        ]),
                        role: ct::BetaMessageRole::Assistant,
                    });
                }
                ot::ResponseInputItem::FunctionCallOutput(tool_result) => {
                    let output_text =
                        openai_function_call_output_content_to_text(&tool_result.output);
                    messages.push(ct::BetaMessageParam {
                        content: ct::BetaMessageContent::Blocks(vec![
                            ct::BetaContentBlockParam::ToolResult(ct::BetaToolResultBlockParam {
                                tool_use_id: tool_result.call_id,
                                type_: ct::BetaToolResultBlockType::ToolResult,
                                cache_control: None,
                                content: if output_text.is_empty() {
                                    None
                                } else {
                                    Some(ct::BetaToolResultBlockParamContent::Text(output_text))
                                },
                                is_error: None,
                            }),
                        ]),
                        role: ct::BetaMessageRole::User,
                    });
                }
                ot::ResponseInputItem::ReasoningItem(reasoning) => {
                    let mut thinking = openai_reasoning_summary_to_text(&reasoning.summary);
                    if thinking.is_empty()
                        && let Some(encrypted) = reasoning.encrypted_content
                    {
                        thinking = encrypted;
                    }

                    if !thinking.is_empty() {
                        messages.push(ct::BetaMessageParam {
                            content: ct::BetaMessageContent::Blocks(vec![
                                ct::BetaContentBlockParam::Thinking(ct::BetaThinkingBlockParam {
                                    signature: reasoning.id,
                                    thinking,
                                    type_: ct::BetaThinkingBlockType::Thinking,
                                }),
                            ]),
                            role: ct::BetaMessageRole::Assistant,
                        });
                    }
                }
                other => {
                    let text = format!("{other:?}");
                    if !text.is_empty() {
                        messages.push(ct::BetaMessageParam {
                            content: ct::BetaMessageContent::Text(text),
                            role: ct::BetaMessageRole::User,
                        });
                    }
                }
            }
        }

        let disable_parallel_tool_use = parallel_disable(body.parallel_tool_calls);
        let tool_choice = openai_tool_choice_to_claude(body.tool_choice, disable_parallel_tool_use);
        let thinking = openai_reasoning_to_claude(body.reasoning.clone());

        let output_effort = body
            .text
            .as_ref()
            .and_then(|text| text.verbosity.as_ref())
            .map(|verbosity| match verbosity {
                ot::ResponseTextVerbosity::Low => ct::BetaOutputEffort::Low,
                ot::ResponseTextVerbosity::Medium => ct::BetaOutputEffort::Medium,
                ot::ResponseTextVerbosity::High => ct::BetaOutputEffort::High,
            });

        let output_format = body
            .text
            .as_ref()
            .and_then(|text| text.format.as_ref())
            .and_then(|format| match format {
                ot::ResponseTextFormatConfig::JsonSchema(schema) => {
                    Some(ct::BetaJsonOutputFormat {
                        schema: schema.schema.clone(),
                        type_: ct::BetaJsonOutputFormatType::JsonSchema,
                    })
                }
                ot::ResponseTextFormatConfig::JsonObject(_) => Some(ct::BetaJsonOutputFormat {
                    schema: serde_json::from_str::<ct::JsonObject>(r#"{"type":"object"}"#)
                        .unwrap_or_default(),
                    type_: ct::BetaJsonOutputFormatType::JsonSchema,
                }),
                _ => None,
            });

        let output_config = if output_effort.is_some() || output_format.is_some() {
            Some(ct::BetaOutputConfig {
                effort: output_effort,
                format: output_format.clone(),
            })
        } else {
            None
        };

        let context_management = {
            let mut edits = Vec::new();

            if let Some(entries) = body.context_management {
                for entry in entries {
                    if entry.type_ == ResponseContextManagementType::Compaction {
                        edits.push(ct::BetaContextManagementEdit::Compact(
                            ct::BetaCompact20260112Edit {
                                type_: ct::BetaCompactType::Compact20260112,
                                instructions: None,
                                pause_after_compaction: None,
                                trigger: entry.compact_threshold.map(|value| {
                                    ct::BetaInputTokensTrigger {
                                        type_: ct::BetaInputTokensCounterType::InputTokens,
                                        value,
                                    }
                                }),
                            },
                        ));
                    }
                }
            }

            if matches!(body.truncation, Some(ot::ResponseTruncation::Auto)) && edits.is_empty() {
                edits.push(ct::BetaContextManagementEdit::Compact(
                    ct::BetaCompact20260112Edit {
                        type_: ct::BetaCompactType::Compact20260112,
                        instructions: None,
                        pause_after_compaction: None,
                        trigger: None,
                    },
                ));
            }

            if edits.is_empty() {
                None
            } else {
                Some(ct::BetaContextManagementConfig { edits: Some(edits) })
            }
        };

        let mut converted_tools = Vec::new();
        let mut mcp_servers = Vec::new();
        if let Some(tools) = body.tools {
            for tool in tools {
                match tool {
                    ot::ResponseTool::Function(tool) => {
                        converted_tools.push(tool_from_function(tool))
                    }
                    ot::ResponseTool::Custom(tool) => {
                        converted_tools.push(ct::BetaToolUnion::Custom(ct::BetaTool {
                            input_schema: ct::BetaToolInputSchema {
                                type_: ct::BetaToolInputSchemaType::Object,
                                properties: None,
                                required: None,
                                extra_fields: Default::default(),
                            },
                            name: tool.name,
                            common: ct::BetaToolCommonFields::default(),
                            description: tool.description,
                            eager_input_streaming: None,
                            type_: Some(ct::BetaCustomToolType::Custom),
                        }));
                    }
                    ot::ResponseTool::CodeInterpreter(_)
                    | ot::ResponseTool::LocalShell(_)
                    | ot::ResponseTool::Shell(_)
                    | ot::ResponseTool::ApplyPatch(_) => {
                        converted_tools.push(ct::BetaToolUnion::CodeExecution20250825(
                            ct::BetaCodeExecutionTool20250825 {
                                name: ct::BetaCodeExecutionToolName::CodeExecution,
                                type_: ct::BetaCodeExecutionTool20250825Type::CodeExecution20250825,
                                common: ct::BetaToolCommonFields::default(),
                            },
                        ));
                    }
                    ot::ResponseTool::Computer(tool) => {
                        converted_tools.push(ct::BetaToolUnion::ComputerUse20251124(
                            ct::BetaToolComputerUse20251124 {
                                display_height_px: tool.display_height,
                                display_width_px: tool.display_width,
                                name: ct::BetaComputerToolName::Computer,
                                type_: ct::BetaToolComputerUse20251124Type::Computer20251124,
                                common: ct::BetaToolCommonFields::default(),
                                display_number: None,
                                enable_zoom: None,
                            },
                        ));
                    }
                    ot::ResponseTool::WebSearch(tool) => {
                        converted_tools.push(ct::BetaToolUnion::WebSearch20250305(
                            ct::BetaWebSearchTool20250305 {
                                name: ct::BetaWebSearchToolName::WebSearch,
                                type_: ct::BetaWebSearchTool20250305Type::WebSearch20250305,
                                common: ct::BetaToolCommonFields::default(),
                                allowed_domains: tool.filters.and_then(|f| f.allowed_domains),
                                blocked_domains: None,
                                max_uses: None,
                                user_location: tool.user_location.map(|location| {
                                    ct::BetaWebSearchUserLocation {
                                        type_: ct::BetaWebSearchUserLocationType::Approximate,
                                        city: location.city,
                                        country: location.country,
                                        region: location.region,
                                        timezone: location.timezone,
                                    }
                                }),
                            },
                        ));
                    }
                    ot::ResponseTool::WebSearchPreview(tool) => {
                        converted_tools.push(ct::BetaToolUnion::WebSearch20250305(
                            ct::BetaWebSearchTool20250305 {
                                name: ct::BetaWebSearchToolName::WebSearch,
                                type_: ct::BetaWebSearchTool20250305Type::WebSearch20250305,
                                common: ct::BetaToolCommonFields::default(),
                                allowed_domains: None,
                                blocked_domains: None,
                                max_uses: None,
                                user_location: tool.user_location.map(|location| {
                                    ct::BetaWebSearchUserLocation {
                                        type_: ct::BetaWebSearchUserLocationType::Approximate,
                                        city: location.city,
                                        country: location.country,
                                        region: location.region,
                                        timezone: location.timezone,
                                    }
                                }),
                            },
                        ));
                    }
                    ot::ResponseTool::FileSearch(_) => {
                        converted_tools.push(ct::BetaToolUnion::ToolSearchBm25_20251119(
                            ct::BetaToolSearchToolBm25_20251119 {
                                name: ct::BetaToolSearchToolBm25Name::ToolSearchToolBm25,
                                type_: ct::BetaToolSearchToolBm25Type::ToolSearchToolBm2520251119,
                                common: ct::BetaToolCommonFields::default(),
                            },
                        ));
                    }
                    ot::ResponseTool::Mcp(tool) => {
                        if let Some(server) = openai_mcp_tool_to_server(&tool) {
                            mcp_servers.push(server);
                        }
                        converted_tools.push(ct::BetaToolUnion::McpToolset(ct::BetaMcpToolset {
                            mcp_server_name: tool.server_label,
                            type_: ct::BetaMcpToolsetType::McpToolset,
                            cache_control: None,
                            configs: mcp_allowed_tools_to_configs(tool.allowed_tools.as_ref()),
                            default_config: None,
                        }));
                    }
                    ot::ResponseTool::ImageGeneration(_) => {}
                }
            }
        }

        let service_tier = match body.service_tier.as_ref() {
            Some(ResponseServiceTier::Auto) => Some(BetaServiceTierParam::Auto),
            Some(
                ResponseServiceTier::Default
                | ResponseServiceTier::Flex
                | ResponseServiceTier::Scale
                | ResponseServiceTier::Priority,
            ) => Some(BetaServiceTierParam::StandardOnly),
            None => None,
        };
        let speed = match body.service_tier.as_ref() {
            Some(ResponseServiceTier::Priority) => Some(BetaSpeed::Fast),
            _ => None,
        };

        let metadata_user_id = body.user.or_else(|| {
            body.metadata
                .as_ref()
                .and_then(|map| map.get("user_id").cloned())
        });
        let metadata = metadata_user_id.map(|user_id| BetaMetadata {
            user_id: Some(user_id),
        });

        let system = body.instructions.and_then(|text| {
            if text.is_empty() {
                None
            } else {
                Some(ct::BetaSystemPrompt::Text(text))
            }
        });

        Ok(ClaudeCreateMessageRequest {
            method: ClaudeHttpMethod::Post,
            path: PathParameters::default(),
            query: QueryParameters::default(),
            headers: RequestHeaders::default(),
            body: RequestBody {
                max_tokens: body.max_output_tokens.unwrap_or(8_192),
                messages,
                model: Model::Custom(body.model.unwrap_or_default()),
                container: None,
                context_management,
                inference_geo: None,
                mcp_servers: if mcp_servers.is_empty() {
                    None
                } else {
                    Some(mcp_servers)
                },
                metadata,
                cache_control: None,
                output_config,
                output_format,
                service_tier,
                speed,
                stop_sequences: None,
                stream: body.stream,
                system,
                temperature: body.temperature,
                thinking,
                tool_choice,
                tools: if converted_tools.is_empty() {
                    None
                } else {
                    Some(converted_tools)
                },
                top_k: None,
                top_p: body.top_p,
            },
        })
    }
}
