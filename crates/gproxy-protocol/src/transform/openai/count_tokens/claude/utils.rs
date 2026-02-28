use std::collections::BTreeMap;

use crate::claude::count_tokens::types as ct;
use crate::openai::count_tokens::types as ot;

fn text_block(text: String) -> ct::BetaContentBlockParam {
    ct::BetaContentBlockParam::Text(ct::BetaTextBlockParam {
        text,
        type_: ct::BetaTextBlockType::Text,
        cache_control: None,
        citations: None,
    })
}

fn parse_data_url_to_image_source(url: &str) -> Option<ct::BetaImageSource> {
    if !url.starts_with("data:") {
        return None;
    }

    let data_index = url.find(";base64,")?;
    let mime = &url[5..data_index];
    let data = &url[(data_index + ";base64,".len())..];

    let media_type = match mime {
        "image/jpeg" => ct::BetaImageMediaType::ImageJpeg,
        "image/png" => ct::BetaImageMediaType::ImagePng,
        "image/gif" => ct::BetaImageMediaType::ImageGif,
        "image/webp" => ct::BetaImageMediaType::ImageWebp,
        _ => return None,
    };

    Some(ct::BetaImageSource::Base64(ct::BetaBase64ImageSource {
        data: data.to_string(),
        media_type,
        type_: ct::BetaBase64SourceType::Base64,
    }))
}

fn openai_content_to_claude_block(
    content: ot::ResponseInputContent,
) -> Option<ct::BetaContentBlockParam> {
    match content {
        ot::ResponseInputContent::Text(part) => Some(text_block(part.text)),
        ot::ResponseInputContent::Image(part) => {
            if let Some(file_id) = part.file_id {
                return Some(ct::BetaContentBlockParam::Image(ct::BetaImageBlockParam {
                    source: ct::BetaImageSource::File(ct::BetaFileImageSource {
                        file_id,
                        type_: ct::BetaFileSourceType::File,
                    }),
                    type_: ct::BetaImageBlockType::Image,
                    cache_control: None,
                }));
            }
            if let Some(image_url) = part.image_url {
                if let Some(source) = parse_data_url_to_image_source(&image_url) {
                    return Some(ct::BetaContentBlockParam::Image(ct::BetaImageBlockParam {
                        source,
                        type_: ct::BetaImageBlockType::Image,
                        cache_control: None,
                    }));
                }
                if !image_url.is_empty() {
                    return Some(ct::BetaContentBlockParam::Image(ct::BetaImageBlockParam {
                        source: ct::BetaImageSource::Url(ct::BetaUrlImageSource {
                            type_: ct::BetaUrlSourceType::Url,
                            url: image_url,
                        }),
                        type_: ct::BetaImageBlockType::Image,
                        cache_control: None,
                    }));
                }
            }
            None
        }
        ot::ResponseInputContent::File(part) => {
            if let Some(file_url) = part.file_url {
                return Some(text_block(file_url));
            }
            if let Some(file_id) = part.file_id {
                return Some(text_block(format!("file_id:{file_id}")));
            }
            if let Some(filename) = part.filename {
                return Some(text_block(filename));
            }
            part.file_data.map(text_block)
        }
    }
}

pub fn openai_message_content_to_claude(
    content: ot::ResponseInputMessageContent,
) -> ct::BetaMessageContent {
    match content {
        ot::ResponseInputMessageContent::Text(text) => ct::BetaMessageContent::Text(text),
        ot::ResponseInputMessageContent::List(parts) => {
            let blocks = parts
                .into_iter()
                .filter_map(openai_content_to_claude_block)
                .collect::<Vec<_>>();

            if blocks.is_empty() {
                ct::BetaMessageContent::Text(String::new())
            } else {
                ct::BetaMessageContent::Blocks(blocks)
            }
        }
    }
}

pub fn openai_role_to_claude(role: ot::ResponseInputMessageRole) -> ct::BetaMessageRole {
    match role {
        ot::ResponseInputMessageRole::Assistant => ct::BetaMessageRole::Assistant,
        ot::ResponseInputMessageRole::User
        | ot::ResponseInputMessageRole::System
        | ot::ResponseInputMessageRole::Developer => ct::BetaMessageRole::User,
    }
}

pub fn openai_reasoning_to_claude(
    reasoning: Option<ot::ResponseReasoning>,
) -> Option<ct::BetaThinkingConfigParam> {
    let effort = reasoning.and_then(|config| config.effort)?;
    Some(match effort {
        ot::ResponseReasoningEffort::None => {
            ct::BetaThinkingConfigParam::Disabled(ct::BetaThinkingConfigDisabled {
                type_: ct::BetaThinkingConfigDisabledType::Disabled,
            })
        }
        ot::ResponseReasoningEffort::Minimal => {
            ct::BetaThinkingConfigParam::Enabled(ct::BetaThinkingConfigEnabled {
                budget_tokens: 10_000,
                type_: ct::BetaThinkingConfigEnabledType::Enabled,
            })
        }
        ot::ResponseReasoningEffort::Low => {
            ct::BetaThinkingConfigParam::Enabled(ct::BetaThinkingConfigEnabled {
                budget_tokens: 20_000,
                type_: ct::BetaThinkingConfigEnabledType::Enabled,
            })
        }
        ot::ResponseReasoningEffort::Medium => {
            ct::BetaThinkingConfigParam::Enabled(ct::BetaThinkingConfigEnabled {
                budget_tokens: 50_000,
                type_: ct::BetaThinkingConfigEnabledType::Enabled,
            })
        }
        ot::ResponseReasoningEffort::High => {
            ct::BetaThinkingConfigParam::Enabled(ct::BetaThinkingConfigEnabled {
                budget_tokens: 100_000,
                type_: ct::BetaThinkingConfigEnabledType::Enabled,
            })
        }
        ot::ResponseReasoningEffort::XHigh => {
            ct::BetaThinkingConfigParam::Enabled(ct::BetaThinkingConfigEnabled {
                budget_tokens: 190_000,
                type_: ct::BetaThinkingConfigEnabledType::Enabled,
            })
        }
    })
}

pub fn parallel_disable(parallel_tool_calls: Option<bool>) -> Option<bool> {
    parallel_tool_calls.map(|enabled| !enabled)
}

pub fn openai_tool_choice_to_claude(
    tool_choice: Option<ot::ResponseToolChoice>,
    disable_parallel_tool_use: Option<bool>,
) -> Option<ct::BetaToolChoice> {
    match tool_choice {
        Some(ot::ResponseToolChoice::Options(ot::ResponseToolChoiceOptions::Auto)) => {
            Some(ct::BetaToolChoice::Auto(ct::BetaToolChoiceAuto {
                type_: ct::BetaToolChoiceAutoType::Auto,
                disable_parallel_tool_use,
            }))
        }
        Some(ot::ResponseToolChoice::Options(ot::ResponseToolChoiceOptions::Required)) => {
            Some(ct::BetaToolChoice::Any(ct::BetaToolChoiceAny {
                type_: ct::BetaToolChoiceAnyType::Any,
                disable_parallel_tool_use,
            }))
        }
        Some(ot::ResponseToolChoice::Options(ot::ResponseToolChoiceOptions::None)) => {
            Some(ct::BetaToolChoice::None(ct::BetaToolChoiceNone {
                type_: ct::BetaToolChoiceNoneType::None,
            }))
        }
        Some(ot::ResponseToolChoice::Function(tool)) => {
            Some(ct::BetaToolChoice::Tool(ct::BetaToolChoiceTool {
                name: tool.name,
                type_: ct::BetaToolChoiceToolType::Tool,
                disable_parallel_tool_use,
            }))
        }
        Some(ot::ResponseToolChoice::Custom(tool)) => {
            Some(ct::BetaToolChoice::Tool(ct::BetaToolChoiceTool {
                name: tool.name,
                type_: ct::BetaToolChoiceToolType::Tool,
                disable_parallel_tool_use,
            }))
        }
        Some(ot::ResponseToolChoice::Mcp(tool)) => {
            if let Some(name) = tool.name {
                Some(ct::BetaToolChoice::Tool(ct::BetaToolChoiceTool {
                    name,
                    type_: ct::BetaToolChoiceToolType::Tool,
                    disable_parallel_tool_use,
                }))
            } else {
                Some(ct::BetaToolChoice::Any(ct::BetaToolChoiceAny {
                    type_: ct::BetaToolChoiceAnyType::Any,
                    disable_parallel_tool_use,
                }))
            }
        }
        Some(ot::ResponseToolChoice::Allowed(choice)) => match choice.mode {
            ot::ResponseToolChoiceAllowedMode::Auto => {
                Some(ct::BetaToolChoice::Auto(ct::BetaToolChoiceAuto {
                    type_: ct::BetaToolChoiceAutoType::Auto,
                    disable_parallel_tool_use,
                }))
            }
            ot::ResponseToolChoiceAllowedMode::Required => {
                Some(ct::BetaToolChoice::Any(ct::BetaToolChoiceAny {
                    type_: ct::BetaToolChoiceAnyType::Any,
                    disable_parallel_tool_use,
                }))
            }
        },
        Some(ot::ResponseToolChoice::Types(_))
        | Some(ot::ResponseToolChoice::ApplyPatch(_))
        | Some(ot::ResponseToolChoice::Shell(_)) => {
            Some(ct::BetaToolChoice::Any(ct::BetaToolChoiceAny {
                type_: ct::BetaToolChoiceAnyType::Any,
                disable_parallel_tool_use,
            }))
        }
        None => None,
    }
}

pub fn mcp_allowed_tools_to_configs(
    allowed_tools: Option<&ot::ResponseMcpAllowedTools>,
) -> Option<BTreeMap<String, ct::BetaMcpToolConfig>> {
    let names = match allowed_tools {
        Some(ot::ResponseMcpAllowedTools::ToolNames(names)) => names.clone(),
        Some(ot::ResponseMcpAllowedTools::Filter(filter)) => {
            filter.tool_names.clone().unwrap_or_default()
        }
        None => Vec::new(),
    };

    let mut configs = BTreeMap::new();
    for name in names {
        configs.insert(
            name,
            ct::BetaMcpToolConfig {
                defer_loading: None,
                enabled: Some(true),
            },
        );
    }

    if configs.is_empty() {
        None
    } else {
        Some(configs)
    }
}

pub fn openai_mcp_tool_to_server(
    tool: &ot::ResponseMcpTool,
) -> Option<ct::BetaRequestMcpServerUrlDefinition> {
    let url = tool.server_url.clone()?;
    let allowed_tools = match &tool.allowed_tools {
        Some(ot::ResponseMcpAllowedTools::ToolNames(names)) => Some(names.clone()),
        Some(ot::ResponseMcpAllowedTools::Filter(filter)) => filter.tool_names.clone(),
        None => None,
    };

    Some(ct::BetaRequestMcpServerUrlDefinition {
        name: tool.server_label.clone(),
        type_: ct::BetaRequestMcpServerType::Url,
        url,
        authorization_token: tool.authorization.clone(),
        tool_configuration: Some(ct::BetaRequestMcpServerToolConfiguration {
            allowed_tools,
            enabled: Some(true),
        }),
    })
}

pub fn tool_from_function(tool: ot::ResponseFunctionTool) -> ct::BetaToolUnion {
    let input_schema = function_parameters_to_tool_input_schema(tool.parameters);
    ct::BetaToolUnion::Custom(ct::BetaTool {
        input_schema,
        name: tool.name,
        common: ct::BetaToolCommonFields {
            strict: tool.strict,
            ..ct::BetaToolCommonFields::default()
        },
        description: tool.description,
        eager_input_streaming: None,
        type_: None,
    })
}

fn function_parameters_to_tool_input_schema(
    mut parameters: ot::JsonObject,
) -> ct::BetaToolInputSchema {
    let required = parameters.remove("required").and_then(|value| match value {
        serde_json::Value::Array(items) => Some(
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>(),
        )
        .filter(|items| !items.is_empty()),
        _ => None,
    });

    let properties = parameters
        .remove("properties")
        .as_ref()
        .and_then(json_object_to_btree);

    // Keep "type" normalized to object in the typed field.
    let _ = parameters.remove("type");

    // Preserve the rest of the JSON Schema payload (e.g. additionalProperties, $defs, oneOf...).
    let mut extra_fields = parameters;

    let properties = properties.or_else(|| {
        let fallback_keys = extra_fields
            .iter()
            .filter(|(key, _)| !is_json_schema_keyword(key))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();

        if fallback_keys.is_empty() {
            return None;
        }

        let fallback = fallback_keys
            .iter()
            .filter_map(|key| extra_fields.remove(key).map(|value| (key.clone(), value)))
            .collect::<ct::JsonObject>();

        if fallback.is_empty() {
            None
        } else {
            Some(fallback)
        }
    });

    ct::BetaToolInputSchema {
        type_: ct::BetaToolInputSchemaType::Object,
        properties,
        required,
        extra_fields,
    }
}

fn is_json_schema_keyword(key: &str) -> bool {
    matches!(
        key,
        "$schema"
            | "$id"
            | "$defs"
            | "definitions"
            | "$ref"
            | "type"
            | "properties"
            | "required"
            | "additionalProperties"
            | "patternProperties"
            | "propertyNames"
            | "unevaluatedProperties"
            | "items"
            | "prefixItems"
            | "contains"
            | "minContains"
            | "maxContains"
            | "allOf"
            | "anyOf"
            | "oneOf"
            | "not"
            | "if"
            | "then"
            | "else"
            | "dependentSchemas"
            | "dependentRequired"
            | "const"
            | "enum"
            | "format"
            | "default"
            | "title"
            | "description"
            | "examples"
            | "readOnly"
            | "writeOnly"
            | "deprecated"
            | "nullable"
            | "minimum"
            | "maximum"
            | "exclusiveMinimum"
            | "exclusiveMaximum"
            | "multipleOf"
            | "minLength"
            | "maxLength"
            | "pattern"
            | "minItems"
            | "maxItems"
            | "uniqueItems"
            | "minProperties"
            | "maxProperties"
            | "contentEncoding"
            | "contentMediaType"
            | "contentSchema"
    )
}

fn json_object_to_btree(value: &serde_json::Value) -> Option<ct::JsonObject> {
    let serde_json::Value::Object(map) = value else {
        return None;
    };
    Some(
        map.iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<ct::JsonObject>(),
    )
}
