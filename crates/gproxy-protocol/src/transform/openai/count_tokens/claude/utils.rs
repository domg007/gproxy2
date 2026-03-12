use std::collections::{BTreeMap, BTreeSet};

use crate::claude::count_tokens::types as ct;
use crate::openai::count_tokens::types as ot;

const CLAUDE_TOOL_USE_ID_PREFIX: &str = "toolu_";
const CLAUDE_SERVER_TOOL_USE_ID_PREFIX: &str = "srvtoolu_";

fn claude_tool_use_id_matches(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix).is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    })
}

fn sanitize_claude_tool_use_suffix(id: &str) -> String {
    let mut suffix = String::new();
    let mut previous_was_underscore = false;

    for ch in id.chars() {
        let mapped = if ch.is_ascii_alphanumeric() || ch == '_' {
            ch
        } else {
            '_'
        };

        if mapped == '_' {
            if suffix.is_empty() || previous_was_underscore {
                continue;
            }
            previous_was_underscore = true;
        } else {
            previous_was_underscore = false;
        }

        suffix.push(mapped);
    }

    while suffix.ends_with('_') {
        suffix.pop();
    }

    if suffix.is_empty() {
        "generated".to_string()
    } else {
        suffix
    }
}

fn normalize_claude_tool_use_id(
    mappings: &mut BTreeMap<String, String>,
    used_ids: &mut BTreeSet<String>,
    original: String,
    prefix: &str,
) -> String {
    if let Some(existing) = mappings.get(&original) {
        return existing.clone();
    }

    let base = if claude_tool_use_id_matches(&original, prefix) {
        original.clone()
    } else {
        let raw_suffix = original.strip_prefix(prefix).unwrap_or(&original);
        format!("{prefix}{}", sanitize_claude_tool_use_suffix(raw_suffix))
    };

    let mut candidate = base.clone();
    let mut suffix = 1usize;
    while used_ids.contains(&candidate) {
        candidate = format!("{base}_{suffix}");
        suffix += 1;
    }

    mappings.insert(original, candidate.clone());
    used_ids.insert(candidate.clone());
    candidate
}

#[derive(Debug, Default)]
pub struct ClaudeToolUseIdMapper {
    tool_use_ids: BTreeMap<String, String>,
    used_tool_use_ids: BTreeSet<String>,
    server_tool_use_ids: BTreeMap<String, String>,
    used_server_tool_use_ids: BTreeSet<String>,
}

impl ClaudeToolUseIdMapper {
    pub fn tool_use_id(&mut self, original: impl Into<String>) -> String {
        normalize_claude_tool_use_id(
            &mut self.tool_use_ids,
            &mut self.used_tool_use_ids,
            original.into(),
            CLAUDE_TOOL_USE_ID_PREFIX,
        )
    }

    pub fn server_tool_use_id(&mut self, original: impl Into<String>) -> String {
        normalize_claude_tool_use_id(
            &mut self.server_tool_use_ids,
            &mut self.used_server_tool_use_ids,
            original.into(),
            CLAUDE_SERVER_TOOL_USE_ID_PREFIX,
        )
    }
}

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

pub fn response_input_content_to_claude_block(
    content: ot::ResponseInputContent,
) -> Option<ct::BetaContentBlockParam> {
    openai_content_to_claude_block(content)
}

pub fn response_input_contents_to_tool_result_content(
    parts: Vec<ot::ResponseInputContent>,
) -> Option<ct::BetaToolResultBlockParamContent> {
    let mut text_parts = Vec::new();
    let mut content_blocks = Vec::new();

    for part in parts {
        match openai_content_to_claude_block(part)? {
            ct::BetaContentBlockParam::Text(block) => text_parts.push(block.text),
            ct::BetaContentBlockParam::Image(block) => {
                content_blocks.push(ct::BetaToolResultContentBlockParam::Image(block))
            }
            ct::BetaContentBlockParam::SearchResult(block) => {
                content_blocks.push(ct::BetaToolResultContentBlockParam::SearchResult(block))
            }
            ct::BetaContentBlockParam::RequestDocument(block) => {
                content_blocks.push(ct::BetaToolResultContentBlockParam::Document(block))
            }
            _ => return None,
        }
    }

    if !content_blocks.is_empty() {
        if !text_parts.is_empty() {
            content_blocks.insert(
                0,
                ct::BetaToolResultContentBlockParam::Text(ct::BetaTextBlockParam {
                    text: text_parts.join("\n"),
                    type_: ct::BetaTextBlockType::Text,
                    cache_control: None,
                    citations: None,
                }),
            );
        }
        Some(ct::BetaToolResultBlockParamContent::Blocks(content_blocks))
    } else if text_parts.is_empty() {
        None
    } else {
        Some(ct::BetaToolResultBlockParamContent::Text(
            text_parts.join("\n"),
        ))
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
    max_tokens: Option<u64>,
) -> Option<ct::BetaThinkingConfigParam> {
    const MIN_BUDGET_TOKENS: u64 = 1_024;

    fn effort_ratio(effort: &ot::ResponseReasoningEffort) -> (u64, u64) {
        match effort {
            ot::ResponseReasoningEffort::Minimal => (1, 8),
            ot::ResponseReasoningEffort::Low => (1, 4),
            ot::ResponseReasoningEffort::Medium => (1, 2),
            ot::ResponseReasoningEffort::High => (3, 4),
            ot::ResponseReasoningEffort::XHigh => (19, 20),
            ot::ResponseReasoningEffort::None => (0, 1),
        }
    }

    fn budget_for_effort(effort: &ot::ResponseReasoningEffort, max_tokens: u64) -> Option<u64> {
        if max_tokens < MIN_BUDGET_TOKENS {
            return None;
        }
        let (num, den) = effort_ratio(effort);
        let target = max_tokens.saturating_mul(num) / den;
        let upper = max_tokens.saturating_sub(1);
        if upper < MIN_BUDGET_TOKENS {
            return None;
        }
        Some(target.clamp(MIN_BUDGET_TOKENS, upper))
    }

    let effort = reasoning.and_then(|config| config.effort)?;
    if !matches!(effort, ot::ResponseReasoningEffort::None)
        && max_tokens.is_some_and(|tokens| tokens < MIN_BUDGET_TOKENS)
    {
        return Some(ct::BetaThinkingConfigParam::Disabled(
            ct::BetaThinkingConfigDisabled {
                type_: ct::BetaThinkingConfigDisabledType::Disabled,
            },
        ));
    }
    Some(match effort {
        ot::ResponseReasoningEffort::None => {
            ct::BetaThinkingConfigParam::Disabled(ct::BetaThinkingConfigDisabled {
                type_: ct::BetaThinkingConfigDisabledType::Disabled,
            })
        }
        ot::ResponseReasoningEffort::Minimal
        | ot::ResponseReasoningEffort::Low
        | ot::ResponseReasoningEffort::Medium
        | ot::ResponseReasoningEffort::High
        | ot::ResponseReasoningEffort::XHigh => {
            if let Some(max_tokens) = max_tokens {
                match budget_for_effort(&effort, max_tokens) {
                    Some(budget_tokens) => {
                        ct::BetaThinkingConfigParam::Enabled(ct::BetaThinkingConfigEnabled {
                            budget_tokens,
                            type_: ct::BetaThinkingConfigEnabledType::Enabled,
                        })
                    }
                    None => ct::BetaThinkingConfigParam::Disabled(ct::BetaThinkingConfigDisabled {
                        type_: ct::BetaThinkingConfigDisabledType::Disabled,
                    }),
                }
            } else {
                ct::BetaThinkingConfigParam::Adaptive(ct::BetaThinkingConfigAdaptive {
                    type_: ct::BetaThinkingConfigAdaptiveType::Adaptive,
                })
            }
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
        Some(ot::ResponseToolChoice::Types(tool)) => {
            let name = match tool.type_ {
                ot::ResponseToolChoiceBuiltinType::FileSearch => "tool_search_tool_bm25",
                ot::ResponseToolChoiceBuiltinType::Computer
                | ot::ResponseToolChoiceBuiltinType::ComputerUsePreview
                | ot::ResponseToolChoiceBuiltinType::ComputerUse => "computer",
                ot::ResponseToolChoiceBuiltinType::WebSearchPreview
                | ot::ResponseToolChoiceBuiltinType::WebSearchPreview20250311 => "web_search",
                ot::ResponseToolChoiceBuiltinType::CodeInterpreter => "code_execution",
                ot::ResponseToolChoiceBuiltinType::ImageGeneration => {
                    return Some(ct::BetaToolChoice::Any(ct::BetaToolChoiceAny {
                        type_: ct::BetaToolChoiceAnyType::Any,
                        disable_parallel_tool_use,
                    }));
                }
            };
            Some(ct::BetaToolChoice::Tool(ct::BetaToolChoiceTool {
                name: name.to_string(),
                type_: ct::BetaToolChoiceToolType::Tool,
                disable_parallel_tool_use,
            }))
        }
        Some(ot::ResponseToolChoice::ApplyPatch(_)) => {
            Some(ct::BetaToolChoice::Tool(ct::BetaToolChoiceTool {
                name: "str_replace_based_edit_tool".to_string(),
                type_: ct::BetaToolChoiceToolType::Tool,
                disable_parallel_tool_use,
            }))
        }
        Some(ot::ResponseToolChoice::Shell(_)) => {
            Some(ct::BetaToolChoice::Tool(ct::BetaToolChoiceTool {
                name: "bash".to_string(),
                type_: ct::BetaToolChoiceToolType::Tool,
                disable_parallel_tool_use,
            }))
        }
        None => None,
    }
}

#[cfg(test)]
mod tool_use_id_tests {
    use super::ClaudeToolUseIdMapper;

    #[test]
    fn tool_use_id_mapper_preserves_valid_ids_and_sanitizes_invalid_ones() {
        let mut mapper = ClaudeToolUseIdMapper::default();

        assert_eq!(mapper.tool_use_id("toolu_valid_1"), "toolu_valid_1");
        assert_eq!(mapper.tool_use_id("call-1"), "toolu_call_1");
        assert_eq!(
            mapper.server_tool_use_id("srvtoolu_valid_2"),
            "srvtoolu_valid_2"
        );
        assert_eq!(mapper.server_tool_use_id("bash 1"), "srvtoolu_bash_1");
    }

    #[test]
    fn tool_use_id_mapper_reuses_existing_mappings_and_avoids_collisions() {
        let mut mapper = ClaudeToolUseIdMapper::default();

        let first = mapper.tool_use_id("call-1");
        let repeated = mapper.tool_use_id("call-1");
        let colliding = mapper.tool_use_id("call_1");
        let server_first = mapper.server_tool_use_id("web/search");
        let server_colliding = mapper.server_tool_use_id("web search");

        assert_eq!(first, "toolu_call_1");
        assert_eq!(repeated, first);
        assert_eq!(colliding, "toolu_call_1_1");
        assert_eq!(server_first, "srvtoolu_web_search");
        assert_eq!(server_colliding, "srvtoolu_web_search_1");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn reasoning(effort: ot::ResponseReasoningEffort) -> Option<ot::ResponseReasoning> {
        Some(ot::ResponseReasoning {
            effort: Some(effort),
            ..ot::ResponseReasoning::default()
        })
    }

    #[test]
    fn reasoning_none_maps_to_disabled() {
        let thinking =
            openai_reasoning_to_claude(reasoning(ot::ResponseReasoningEffort::None), Some(8_192));
        assert!(matches!(
            thinking,
            Some(ct::BetaThinkingConfigParam::Disabled(_))
        ));
    }

    #[test]
    fn reasoning_budget_is_clamped_to_minimum() {
        let thinking = openai_reasoning_to_claude(
            reasoning(ot::ResponseReasoningEffort::Minimal),
            Some(4_096),
        );
        match thinking {
            Some(ct::BetaThinkingConfigParam::Enabled(config)) => {
                assert_eq!(config.budget_tokens, 1_024);
            }
            other => panic!("unexpected thinking config: {other:?}"),
        }
    }

    #[test]
    fn reasoning_budget_never_exceeds_max_tokens() {
        let thinking =
            openai_reasoning_to_claude(reasoning(ot::ResponseReasoningEffort::XHigh), Some(1_500));
        match thinking {
            Some(ct::BetaThinkingConfigParam::Enabled(config)) => {
                assert!(config.budget_tokens <= 1_499);
                assert!(config.budget_tokens >= 1_024);
            }
            other => panic!("unexpected thinking config: {other:?}"),
        }
    }

    #[test]
    fn reasoning_medium_maps_to_budgeted_enabled() {
        let thinking =
            openai_reasoning_to_claude(reasoning(ot::ResponseReasoningEffort::Medium), Some(8_192));
        match thinking {
            Some(ct::BetaThinkingConfigParam::Enabled(config)) => {
                assert_eq!(config.budget_tokens, 4_096);
            }
            other => panic!("unexpected thinking config: {other:?}"),
        }
    }

    #[test]
    fn reasoning_is_disabled_when_max_tokens_below_minimum() {
        let thinking =
            openai_reasoning_to_claude(reasoning(ot::ResponseReasoningEffort::Low), Some(800));
        assert!(matches!(
            thinking,
            Some(ct::BetaThinkingConfigParam::Disabled(_))
        ));
    }

    #[test]
    fn reasoning_budgeted_effort_without_max_tokens_maps_to_adaptive() {
        let thinking =
            openai_reasoning_to_claude(reasoning(ot::ResponseReasoningEffort::High), None);
        assert!(matches!(
            thinking,
            Some(ct::BetaThinkingConfigParam::Adaptive(_))
        ));
    }

    #[test]
    fn reasoning_budgeted_effort_is_disabled_when_max_tokens_is_1024() {
        let thinking =
            openai_reasoning_to_claude(reasoning(ot::ResponseReasoningEffort::High), Some(1_024));
        assert!(matches!(
            thinking,
            Some(ct::BetaThinkingConfigParam::Disabled(_))
        ));
    }

    fn assert_named_tool_choice(
        choice: ot::ResponseToolChoice,
        disable_parallel_tool_use: Option<bool>,
        expected_name: &str,
    ) {
        match openai_tool_choice_to_claude(Some(choice), disable_parallel_tool_use) {
            Some(ct::BetaToolChoice::Tool(tool)) => {
                assert_eq!(tool.name, expected_name);
                assert_eq!(tool.disable_parallel_tool_use, disable_parallel_tool_use);
            }
            other => panic!("unexpected tool choice: {other:?}"),
        }
    }

    #[test]
    fn builtin_tool_choices_map_to_named_claude_tools() {
        assert_named_tool_choice(
            ot::ResponseToolChoice::Types(ot::ResponseToolChoiceTypes {
                type_: ot::ResponseToolChoiceBuiltinType::FileSearch,
            }),
            Some(true),
            "tool_search_tool_bm25",
        );
        assert_named_tool_choice(
            ot::ResponseToolChoice::Types(ot::ResponseToolChoiceTypes {
                type_: ot::ResponseToolChoiceBuiltinType::ComputerUsePreview,
            }),
            Some(false),
            "computer",
        );
        assert_named_tool_choice(
            ot::ResponseToolChoice::Types(ot::ResponseToolChoiceTypes {
                type_: ot::ResponseToolChoiceBuiltinType::WebSearchPreview,
            }),
            None,
            "web_search",
        );
        assert_named_tool_choice(
            ot::ResponseToolChoice::Types(ot::ResponseToolChoiceTypes {
                type_: ot::ResponseToolChoiceBuiltinType::WebSearchPreview20250311,
            }),
            None,
            "web_search",
        );
        assert_named_tool_choice(
            ot::ResponseToolChoice::Types(ot::ResponseToolChoiceTypes {
                type_: ot::ResponseToolChoiceBuiltinType::CodeInterpreter,
            }),
            None,
            "code_execution",
        );
        assert_named_tool_choice(
            ot::ResponseToolChoice::ApplyPatch(ot::ResponseToolChoiceApplyPatch {
                type_: ot::ResponseToolChoiceApplyPatchType::ApplyPatch,
            }),
            None,
            "str_replace_based_edit_tool",
        );
        assert_named_tool_choice(
            ot::ResponseToolChoice::Shell(ot::ResponseToolChoiceShell {
                type_: ot::ResponseToolChoiceShellType::Shell,
            }),
            Some(true),
            "bash",
        );
    }
}
