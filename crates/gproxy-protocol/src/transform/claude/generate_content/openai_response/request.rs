use crate::claude::count_tokens::types::{
    BetaContextManagementEdit, BetaMessageRole, BetaOutputEffort, BetaThinkingConfigParam,
    BetaToolChoice, BetaToolInputSchema, BetaToolInputSchemaType, BetaToolUnion,
};
use crate::claude::create_message::request::ClaudeCreateMessageRequest;
use crate::claude::create_message::types::{BetaServiceTierParam, BetaSpeed};
use crate::openai::count_tokens::types::{
    HttpMethod, ResponseApplyPatchTool, ResponseApplyPatchToolType, ResponseApproximateLocation,
    ResponseApproximateLocationType, ResponseCodeInterpreterContainer, ResponseCodeInterpreterTool,
    ResponseCodeInterpreterToolAuto, ResponseCodeInterpreterToolAutoType,
    ResponseCodeInterpreterToolType, ResponseComputerEnvironment, ResponseComputerTool,
    ResponseComputerToolType, ResponseFormatTextJsonSchemaConfig,
    ResponseFormatTextJsonSchemaConfigType, ResponseFunctionShellTool,
    ResponseFunctionShellToolType, ResponseFunctionTool, ResponseFunctionToolType, ResponseInput,
    ResponseInputItem, ResponseInputMessage, ResponseInputMessageContent, ResponseInputMessageRole,
    ResponseInputMessageType, ResponseMcpAllowedTools, ResponseMcpTool, ResponseMcpToolType,
    ResponseReasoning, ResponseReasoningEffort, ResponseTextConfig, ResponseTextFormatConfig,
    ResponseTextVerbosity, ResponseTool, ResponseToolChoice, ResponseToolChoiceFunction,
    ResponseToolChoiceFunctionType, ResponseToolChoiceOptions, ResponseTruncation,
    ResponseWebSearchFilters, ResponseWebSearchTool, ResponseWebSearchToolType,
};
use crate::openai::create_response::request::{
    OpenAiCreateResponseRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::openai::create_response::types::{
    Metadata, ResponseContextManagementEntry, ResponseContextManagementType, ResponseServiceTier,
};
use crate::transform::claude::generate_content::utils::{
    beta_message_content_to_text, beta_system_prompt_to_text, claude_model_to_string,
};
use crate::transform::utils::TransformError;
use serde_json::{Map, Value};

fn tool_input_schema_to_json_object(
    input_schema: BetaToolInputSchema,
) -> std::collections::BTreeMap<String, Value> {
    let mut parameters = std::collections::BTreeMap::new();
    let schema_type = match input_schema.type_ {
        BetaToolInputSchemaType::Object => "object",
    };
    parameters.insert("type".to_string(), Value::String(schema_type.to_string()));
    if let Some(properties) = input_schema.properties {
        let properties_object = properties.into_iter().collect::<Map<String, Value>>();
        parameters.insert("properties".to_string(), Value::Object(properties_object));
    }
    if let Some(required) = input_schema.required {
        parameters.insert(
            "required".to_string(),
            Value::Array(required.into_iter().map(Value::String).collect()),
        );
    }
    parameters
}

impl TryFrom<ClaudeCreateMessageRequest> for OpenAiCreateResponseRequest {
    type Error = TransformError;

    fn try_from(value: ClaudeCreateMessageRequest) -> Result<Self, TransformError> {
        let body = value.body;
        let model = claude_model_to_string(&body.model);

        let input_items = body
            .messages
            .into_iter()
            .map(|message| {
                ResponseInputItem::Message(ResponseInputMessage {
                    content: ResponseInputMessageContent::Text(beta_message_content_to_text(
                        &message.content,
                    )),
                    role: match message.role {
                        BetaMessageRole::User => ResponseInputMessageRole::User,
                        BetaMessageRole::Assistant => ResponseInputMessageRole::Assistant,
                    },
                    status: None,
                    type_: Some(ResponseInputMessageType::Message),
                })
            })
            .collect::<Vec<_>>();

        let instructions = beta_system_prompt_to_text(body.system);
        let parallel_tool_calls = match body.tool_choice.as_ref() {
            Some(BetaToolChoice::Auto(choice)) => choice.disable_parallel_tool_use.map(|v| !v),
            Some(BetaToolChoice::Any(choice)) => choice.disable_parallel_tool_use.map(|v| !v),
            Some(BetaToolChoice::Tool(choice)) => choice.disable_parallel_tool_use.map(|v| !v),
            Some(BetaToolChoice::None(_)) | None => None,
        };
        let tool_choice = match body.tool_choice {
            Some(BetaToolChoice::Auto(_)) => {
                Some(ResponseToolChoice::Options(ResponseToolChoiceOptions::Auto))
            }
            Some(BetaToolChoice::Any(_)) => Some(ResponseToolChoice::Options(
                ResponseToolChoiceOptions::Required,
            )),
            Some(BetaToolChoice::Tool(choice)) => {
                Some(ResponseToolChoice::Function(ResponseToolChoiceFunction {
                    name: choice.name,
                    type_: ResponseToolChoiceFunctionType::Function,
                }))
            }
            Some(BetaToolChoice::None(_)) => {
                Some(ResponseToolChoice::Options(ResponseToolChoiceOptions::None))
            }
            None => None,
        };
        let reasoning_effort_from_thinking = match body.thinking {
            Some(BetaThinkingConfigParam::Enabled(config)) => Some(if config.budget_tokens == 0 {
                ResponseReasoningEffort::None
            } else if config.budget_tokens <= 4096 {
                ResponseReasoningEffort::Minimal
            } else if config.budget_tokens <= 8192 {
                ResponseReasoningEffort::Low
            } else if config.budget_tokens <= 16384 {
                ResponseReasoningEffort::Medium
            } else if config.budget_tokens <= 32768 {
                ResponseReasoningEffort::High
            } else {
                ResponseReasoningEffort::XHigh
            }),
            Some(BetaThinkingConfigParam::Disabled(_)) => Some(ResponseReasoningEffort::None),
            Some(BetaThinkingConfigParam::Adaptive(_)) => Some(ResponseReasoningEffort::Medium),
            None => None,
        };
        let reasoning = reasoning_effort_from_thinking.map(|effort| ResponseReasoning {
            effort: Some(effort),
            generate_summary: None,
            summary: None,
        });
        let output_schema = body
            .output_config
            .as_ref()
            .and_then(|config| config.format.as_ref())
            .or(body.output_format.as_ref());
        let text_format = output_schema.map(|schema| {
            ResponseTextFormatConfig::JsonSchema(ResponseFormatTextJsonSchemaConfig {
                name: "output".to_string(),
                schema: schema.schema.clone(),
                type_: ResponseFormatTextJsonSchemaConfigType::JsonSchema,
                description: None,
                strict: None,
            })
        });
        let text_verbosity = body
            .output_config
            .as_ref()
            .and_then(|config| config.effort.as_ref())
            .map(|effort| match effort {
                BetaOutputEffort::Low => ResponseTextVerbosity::Low,
                BetaOutputEffort::Medium => ResponseTextVerbosity::Medium,
                BetaOutputEffort::High | BetaOutputEffort::Max => ResponseTextVerbosity::High,
            });
        let text = if text_format.is_some() || text_verbosity.is_some() {
            Some(ResponseTextConfig {
                format: text_format,
                verbosity: text_verbosity,
            })
        } else {
            None
        };
        let context_management = body.context_management.as_ref().and_then(|config| {
            let mut entries = Vec::new();
            if let Some(edits) = config.edits.as_ref() {
                for edit in edits {
                    if let BetaContextManagementEdit::Compact(compact) = edit {
                        entries.push(ResponseContextManagementEntry {
                            type_: ResponseContextManagementType::Compaction,
                            compact_threshold: compact
                                .trigger
                                .as_ref()
                                .map(|trigger| trigger.value),
                        });
                    }
                }
            }

            if entries.is_empty() {
                None
            } else {
                Some(entries)
            }
        });
        let truncation = body
            .context_management
            .as_ref()
            .map(|_| ResponseTruncation::Auto);

        let mut converted_tools = Vec::new();
        if let Some(tools) = body.tools {
            for tool in tools {
                match tool {
                    BetaToolUnion::Custom(tool) => {
                        converted_tools.push(ResponseTool::Function(ResponseFunctionTool {
                            name: tool.name,
                            parameters: tool_input_schema_to_json_object(tool.input_schema),
                            strict: tool.common.strict,
                            type_: ResponseFunctionToolType::Function,
                            description: tool.description,
                        }));
                    }
                    BetaToolUnion::CodeExecution20250522(_)
                    | BetaToolUnion::CodeExecution20250825(_) => {
                        converted_tools.push(ResponseTool::CodeInterpreter(
                            ResponseCodeInterpreterTool {
                                container: ResponseCodeInterpreterContainer::Auto(
                                    ResponseCodeInterpreterToolAuto {
                                        type_: ResponseCodeInterpreterToolAutoType::Auto,
                                        file_ids: None,
                                        memory_limit: None,
                                        network_policy: None,
                                    },
                                ),
                                type_: ResponseCodeInterpreterToolType::CodeInterpreter,
                            },
                        ));
                    }
                    BetaToolUnion::ComputerUse20241022(tool) => {
                        converted_tools.push(ResponseTool::Computer(ResponseComputerTool {
                            display_height: tool.display_height_px,
                            display_width: tool.display_width_px,
                            environment: ResponseComputerEnvironment::Browser,
                            type_: ResponseComputerToolType::ComputerUsePreview,
                        }));
                    }
                    BetaToolUnion::ComputerUse20250124(tool) => {
                        converted_tools.push(ResponseTool::Computer(ResponseComputerTool {
                            display_height: tool.display_height_px,
                            display_width: tool.display_width_px,
                            environment: ResponseComputerEnvironment::Browser,
                            type_: ResponseComputerToolType::ComputerUsePreview,
                        }));
                    }
                    BetaToolUnion::ComputerUse20251124(tool) => {
                        converted_tools.push(ResponseTool::Computer(ResponseComputerTool {
                            display_height: tool.display_height_px,
                            display_width: tool.display_width_px,
                            environment: ResponseComputerEnvironment::Browser,
                            type_: ResponseComputerToolType::ComputerUsePreview,
                        }));
                    }
                    BetaToolUnion::WebSearch20250305(tool) => {
                        converted_tools.push(ResponseTool::WebSearch(ResponseWebSearchTool {
                            type_: ResponseWebSearchToolType::WebSearch,
                            filters: tool.allowed_domains.map(|allowed_domains| {
                                ResponseWebSearchFilters {
                                    allowed_domains: Some(allowed_domains),
                                }
                            }),
                            search_context_size: None,
                            user_location: tool.user_location.map(|location| {
                                ResponseApproximateLocation {
                                    city: location.city,
                                    country: location.country,
                                    region: location.region,
                                    timezone: location.timezone,
                                    type_: Some(ResponseApproximateLocationType::Approximate),
                                }
                            }),
                        }));
                    }
                    BetaToolUnion::WebFetch20250910(tool) => {
                        converted_tools.push(ResponseTool::WebSearch(ResponseWebSearchTool {
                            type_: ResponseWebSearchToolType::WebSearch,
                            filters: tool.allowed_domains.map(|allowed_domains| {
                                ResponseWebSearchFilters {
                                    allowed_domains: Some(allowed_domains),
                                }
                            }),
                            search_context_size: None,
                            user_location: None,
                        }));
                    }
                    BetaToolUnion::Bash20241022(_)
                    | BetaToolUnion::Bash20250124(_)
                    | BetaToolUnion::ToolSearchBm25_20251119(_)
                    | BetaToolUnion::ToolSearchRegex20251119(_) => {
                        converted_tools.push(ResponseTool::Shell(ResponseFunctionShellTool {
                            type_: ResponseFunctionShellToolType::Shell,
                            environment: None,
                        }));
                    }
                    BetaToolUnion::TextEditor20241022(_)
                    | BetaToolUnion::TextEditor20250124(_)
                    | BetaToolUnion::TextEditor20250429(_)
                    | BetaToolUnion::TextEditor20250728(_) => {
                        converted_tools.push(ResponseTool::ApplyPatch(ResponseApplyPatchTool {
                            type_: ResponseApplyPatchToolType::ApplyPatch,
                        }));
                    }
                    BetaToolUnion::McpToolset(tool) => {
                        let allowed_tools = tool.configs.and_then(|configs| {
                            let names = configs
                                .into_iter()
                                .filter_map(|(name, config)| {
                                    if config.enabled.unwrap_or(true) {
                                        Some(name)
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>();
                            if names.is_empty() {
                                None
                            } else {
                                Some(ResponseMcpAllowedTools::ToolNames(names))
                            }
                        });
                        converted_tools.push(ResponseTool::Mcp(ResponseMcpTool {
                            server_label: tool.mcp_server_name,
                            type_: ResponseMcpToolType::Mcp,
                            allowed_tools,
                            authorization: None,
                            connector_id: None,
                            headers: None,
                            require_approval: None,
                            server_description: None,
                            server_url: None,
                        }));
                    }
                    BetaToolUnion::Memory20250818(_) => {}
                }
            }
        }
        if let Some(servers) = body.mcp_servers {
            for server in servers {
                converted_tools.push(ResponseTool::Mcp(ResponseMcpTool {
                    server_label: server.name,
                    type_: ResponseMcpToolType::Mcp,
                    allowed_tools: server
                        .tool_configuration
                        .as_ref()
                        .and_then(|config| config.allowed_tools.clone())
                        .map(ResponseMcpAllowedTools::ToolNames),
                    authorization: server.authorization_token,
                    connector_id: None,
                    headers: None,
                    require_approval: None,
                    server_description: None,
                    server_url: Some(server.url),
                }));
            }
        }
        let tools = if converted_tools.is_empty() {
            None
        } else {
            Some(converted_tools)
        };

        let metadata = if let Some(user_id) = body
            .metadata
            .as_ref()
            .and_then(|value| value.user_id.clone())
        {
            let mut map = Metadata::new();
            map.insert("user_id".to_string(), user_id);
            Some(map)
        } else {
            None
        };
        let service_tier = match body.service_tier {
            Some(BetaServiceTierParam::Auto) => Some(ResponseServiceTier::Auto),
            Some(BetaServiceTierParam::StandardOnly) => Some(ResponseServiceTier::Default),
            None => match body.speed {
                Some(BetaSpeed::Fast) => Some(ResponseServiceTier::Priority),
                Some(BetaSpeed::Standard) | None => None,
            },
        };

        Ok(Self {
            method: HttpMethod::Post,
            path: PathParameters::default(),
            query: QueryParameters::default(),
            headers: RequestHeaders::default(),
            body: RequestBody {
                context_management,
                input: if input_items.is_empty() {
                    None
                } else {
                    Some(ResponseInput::Items(input_items))
                },
                instructions,
                max_output_tokens: Some(body.max_tokens),
                metadata,
                model: Some(model),
                parallel_tool_calls,
                reasoning,
                service_tier,
                stream: body.stream,
                temperature: body.temperature,
                text,
                tool_choice,
                tools,
                top_p: body.top_p,
                truncation,
                ..RequestBody::default()
            },
        })
    }
}
