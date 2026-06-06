use crate::protocol::{claude, gemini, openai};

use super::scalar::{i32_to_u64, u64_to_i32};
use super::util::{json_object, json_value};

pub(in crate::transform::count_tokens) fn openai_generation_config_to_gemini(
    reasoning: Option<openai::ReasoningConfig>,
    text: Option<openai::TextConfig>,
) -> Option<gemini::GenerationConfig> {
    let mut config = gemini::GenerationConfig::default();

    if let Some(reasoning) = reasoning {
        config.thinking_config = Some(gemini::ThinkingConfig {
            include_thoughts: None,
            thinking_budget: None,
            thinking_level: reasoning.effort.map(openai_reasoning_effort_to_gemini),
            extra: Default::default(),
        });
    }

    if let Some(text) = text.and_then(|text| text.format) {
        apply_openai_response_format(&mut config, text);
    }

    non_empty_generation_config(config)
}

fn openai_reasoning_effort_to_gemini(effort: openai::ReasoningEffort) -> gemini::ThinkingLevel {
    let level = match effort {
        openai::ReasoningEffort::None | openai::ReasoningEffort::Minimal => {
            gemini::ThinkingLevelKnown::Minimal
        }
        openai::ReasoningEffort::Low => gemini::ThinkingLevelKnown::Low,
        openai::ReasoningEffort::Medium => gemini::ThinkingLevelKnown::Medium,
        openai::ReasoningEffort::High | openai::ReasoningEffort::XHigh => {
            gemini::ThinkingLevelKnown::High
        }
    };
    gemini::ThinkingLevel::Known(level)
}

fn apply_openai_response_format(
    config: &mut gemini::GenerationConfig,
    format: openai::ResponseFormat,
) {
    match format {
        openai::ResponseFormat::Text(_) => {
            config.response_mime_type = Some(gemini::ResponseMimeType::Known(
                gemini::ResponseMimeTypeKnown::TextPlain,
            ));
        }
        openai::ResponseFormat::JsonObject(_) => {
            config.response_mime_type = Some(gemini::ResponseMimeType::Known(
                gemini::ResponseMimeTypeKnown::ApplicationJson,
            ));
        }
        openai::ResponseFormat::JsonSchema(format) => {
            config.response_mime_type = Some(gemini::ResponseMimeType::Known(
                gemini::ResponseMimeTypeKnown::ApplicationJson,
            ));
            config.response_json_schema = Some(json_value(format.schema));
        }
    }
}

pub(in crate::transform::count_tokens) fn claude_generation_config_to_gemini(
    output_config: Option<claude::OutputConfig>,
    output_format: Option<claude::JsonSchemaFormat>,
    thinking: Option<claude::ThinkingConfig>,
) -> Option<gemini::GenerationConfig> {
    let mut config = gemini::GenerationConfig::default();

    let output_format = output_config
        .as_ref()
        .and_then(|config| config.format.clone())
        .or(output_format);
    if let Some(format) = output_format {
        config.response_mime_type = Some(gemini::ResponseMimeType::Known(
            gemini::ResponseMimeTypeKnown::ApplicationJson,
        ));
        config.response_json_schema = Some(json_value(format.schema));
    }

    if let Some(task_budget) = output_config.and_then(|config| config.task_budget) {
        config.max_output_tokens = Some(u64_to_i32(task_budget.total));
    }

    if let Some(thinking) = thinking {
        config.thinking_config = Some(claude_thinking_to_gemini(thinking));
    }

    non_empty_generation_config(config)
}

pub(in crate::transform::count_tokens) fn claude_service_tier_to_gemini(
    service_tier: Option<claude::RequestServiceTier>,
) -> Option<gemini::ServiceTier> {
    let tier = match service_tier? {
        claude::RequestServiceTier::Known(claude::RequestServiceTierKnown::Auto) => {
            gemini::ServiceTierKnown::Unspecified
        }
        claude::RequestServiceTier::Known(claude::RequestServiceTierKnown::StandardOnly) => {
            gemini::ServiceTierKnown::Standard
        }
        claude::RequestServiceTier::Unknown(_) => gemini::ServiceTierKnown::Standard,
    };
    Some(gemini::ServiceTier::Known(tier))
}

pub(in crate::transform::count_tokens) fn openai_service_tier_to_gemini(
    service_tier: Option<openai::ServiceTier>,
) -> Option<gemini::ServiceTier> {
    let tier = match service_tier? {
        openai::ServiceTier::Auto => gemini::ServiceTierKnown::Unspecified,
        openai::ServiceTier::Default => gemini::ServiceTierKnown::Standard,
        openai::ServiceTier::Flex => gemini::ServiceTierKnown::Flex,
        openai::ServiceTier::Priority => gemini::ServiceTierKnown::Priority,
        openai::ServiceTier::Scale => gemini::ServiceTierKnown::Standard,
    };
    Some(gemini::ServiceTier::Known(tier))
}

fn claude_thinking_to_gemini(thinking: claude::ThinkingConfig) -> gemini::ThinkingConfig {
    match thinking {
        claude::ThinkingConfig::Enabled(config) => gemini::ThinkingConfig {
            include_thoughts: Some(true),
            thinking_budget: Some(u64_to_i32(config.budget_tokens)),
            thinking_level: None,
            extra: Default::default(),
        },
        claude::ThinkingConfig::Disabled(_) => gemini::ThinkingConfig {
            include_thoughts: Some(false),
            thinking_budget: None,
            thinking_level: None,
            extra: Default::default(),
        },
        claude::ThinkingConfig::Adaptive(_) => gemini::ThinkingConfig {
            include_thoughts: Some(true),
            thinking_budget: None,
            thinking_level: None,
            extra: Default::default(),
        },
        claude::ThinkingConfig::Unknown(_) => gemini::ThinkingConfig::default(),
    }
}

fn non_empty_generation_config(
    config: gemini::GenerationConfig,
) -> Option<gemini::GenerationConfig> {
    if config == gemini::GenerationConfig::default() {
        None
    } else {
        Some(config)
    }
}

pub(in crate::transform::count_tokens) fn claude_generation_to_openai_reasoning(
    thinking: Option<claude::ThinkingConfig>,
    output_config: Option<&claude::OutputConfig>,
) -> Option<openai::ReasoningConfig> {
    let effort = output_config
        .and_then(|config| config.effort.clone())
        .map(claude_output_effort_to_openai)
        .or_else(|| thinking.map(claude_thinking_to_openai_effort));

    effort.map(|effort| openai::ReasoningConfig {
        effort: Some(effort),
        summary: None,
        generate_summary: None,
        extra: Default::default(),
    })
}

fn claude_output_effort_to_openai(effort: claude::OutputEffort) -> openai::ReasoningEffort {
    match effort {
        claude::OutputEffort::Known(claude::OutputEffortKnown::Low) => openai::ReasoningEffort::Low,
        claude::OutputEffort::Known(claude::OutputEffortKnown::Medium) => {
            openai::ReasoningEffort::Medium
        }
        claude::OutputEffort::Known(claude::OutputEffortKnown::High) => {
            openai::ReasoningEffort::High
        }
        claude::OutputEffort::Known(claude::OutputEffortKnown::XHigh)
        | claude::OutputEffort::Known(claude::OutputEffortKnown::Max)
        | claude::OutputEffort::Unknown(_) => openai::ReasoningEffort::XHigh,
    }
}

fn claude_thinking_to_openai_effort(thinking: claude::ThinkingConfig) -> openai::ReasoningEffort {
    match thinking {
        claude::ThinkingConfig::Disabled(_) => openai::ReasoningEffort::None,
        claude::ThinkingConfig::Enabled(_) => openai::ReasoningEffort::Medium,
        claude::ThinkingConfig::Adaptive(_) => openai::ReasoningEffort::Medium,
        claude::ThinkingConfig::Unknown(_) => openai::ReasoningEffort::Medium,
    }
}

pub(in crate::transform::count_tokens) fn gemini_generation_to_openai_reasoning(
    generation_config: Option<&gemini::GenerationConfig>,
) -> Option<openai::ReasoningConfig> {
    let thinking = generation_config?.thinking_config.as_ref()?;
    let effort = if thinking.include_thoughts == Some(false) {
        openai::ReasoningEffort::None
    } else {
        thinking
            .thinking_level
            .clone()
            .map(gemini_thinking_level_to_openai)
            .unwrap_or(openai::ReasoningEffort::Medium)
    };
    Some(openai::ReasoningConfig {
        effort: Some(effort),
        summary: None,
        generate_summary: None,
        extra: Default::default(),
    })
}

fn gemini_thinking_level_to_openai(level: gemini::ThinkingLevel) -> openai::ReasoningEffort {
    match level {
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Minimal) => {
            openai::ReasoningEffort::Minimal
        }
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Low) => {
            openai::ReasoningEffort::Low
        }
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Medium)
        | gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::ThinkingLevelUnspecified) => {
            openai::ReasoningEffort::Medium
        }
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::High)
        | gemini::ThinkingLevel::Unknown(_) => openai::ReasoningEffort::High,
    }
}

pub(in crate::transform::count_tokens) fn claude_generation_to_openai_text(
    output_config: Option<&claude::OutputConfig>,
    output_format: Option<claude::JsonSchemaFormat>,
) -> Option<openai::TextConfig> {
    let format = output_config
        .and_then(|config| config.format.clone())
        .or(output_format)?;
    Some(openai::TextConfig {
        format: Some(openai::ResponseFormat::JsonSchema(
            openai::JsonSchemaResponseFormat {
                type_: openai::JsonSchemaResponseFormatType::JsonSchema,
                name: "response".to_owned(),
                schema: json_object(json_value(format.schema)),
                description: None,
                strict: None,
                extra: Default::default(),
            },
        )),
        verbosity: None,
        extra: Default::default(),
    })
}

pub(in crate::transform::count_tokens) fn gemini_generation_to_openai_text(
    generation_config: Option<&gemini::GenerationConfig>,
) -> Option<openai::TextConfig> {
    let config = generation_config?;
    let format = if let Some(schema) = config
        .response_json_schema
        .clone()
        .or_else(|| config.response_schema.clone().map(json_value))
    {
        openai::ResponseFormat::JsonSchema(openai::JsonSchemaResponseFormat {
            type_: openai::JsonSchemaResponseFormatType::JsonSchema,
            name: "response".to_owned(),
            schema: json_object(schema),
            description: None,
            strict: None,
            extra: Default::default(),
        })
    } else if matches!(
        config.response_mime_type,
        Some(gemini::ResponseMimeType::Known(
            gemini::ResponseMimeTypeKnown::ApplicationJson
        ))
    ) {
        openai::ResponseFormat::JsonObject(openai::JsonObjectResponseFormat {
            type_: openai::JsonObjectResponseFormatType::JsonObject,
            extra: Default::default(),
        })
    } else if matches!(
        config.response_mime_type,
        Some(gemini::ResponseMimeType::Known(
            gemini::ResponseMimeTypeKnown::TextPlain
        ))
    ) {
        openai::ResponseFormat::Text(openai::TextResponseFormat {
            type_: openai::TextResponseFormatType::Text,
            extra: Default::default(),
        })
    } else {
        return None;
    };

    Some(openai::TextConfig {
        format: Some(format),
        verbosity: None,
        extra: Default::default(),
    })
}

pub(in crate::transform::count_tokens) fn gemini_generation_to_claude_output_config(
    generation_config: Option<&gemini::GenerationConfig>,
) -> Option<claude::OutputConfig> {
    let config = generation_config?;
    let effort = config
        .thinking_config
        .as_ref()
        .and_then(gemini_thinking_to_claude_output_effort);
    let format = gemini_generation_to_claude_output_format(Some(config));
    let task_budget = config
        .max_output_tokens
        .map(|total| claude::TokenTaskBudget {
            total: i32_to_u64(total),
            type_: claude::TaskBudgetType::Known(claude::TaskBudgetTypeKnown::Tokens),
            remaining: None,
            extra: Default::default(),
        });

    if effort.is_none() && format.is_none() && task_budget.is_none() {
        return None;
    }

    Some(claude::OutputConfig {
        effort,
        format,
        task_budget,
        extra: Default::default(),
    })
}

pub(in crate::transform::count_tokens) fn gemini_generation_to_claude_output_format(
    generation_config: Option<&gemini::GenerationConfig>,
) -> Option<claude::JsonSchemaFormat> {
    let config = generation_config?;
    let schema = config
        .response_json_schema
        .clone()
        .or_else(|| config.private_response_json_schema.clone())
        .or_else(|| {
            config
                .response_format
                .as_ref()
                .and_then(|format| format.text.as_ref())
                .and_then(|format| format.schema.clone())
        })
        .or_else(|| config.response_schema.clone().map(json_value))?;

    Some(claude::JsonSchemaFormat {
        type_: claude::JsonSchemaFormatType::Known(claude::JsonSchemaFormatTypeKnown::JsonSchema),
        schema: json_object(schema),
        extra: Default::default(),
    })
}

pub(in crate::transform::count_tokens) fn gemini_generation_to_claude_thinking(
    generation_config: Option<&gemini::GenerationConfig>,
) -> Option<claude::ThinkingConfig> {
    let thinking = generation_config?.thinking_config.as_ref()?;
    if thinking.include_thoughts == Some(false) {
        return Some(claude::ThinkingConfig::Disabled(claude::ThinkingDisabled {
            type_: claude::ThinkingDisabledType::Disabled,
            extra: Default::default(),
        }));
    }
    if let Some(budget) = thinking.thinking_budget {
        return Some(claude::ThinkingConfig::Enabled(claude::ThinkingEnabled {
            budget_tokens: i32_to_u64(budget),
            type_: claude::ThinkingEnabledType::Enabled,
            display: None,
            extra: Default::default(),
        }));
    }
    Some(claude::ThinkingConfig::Adaptive(claude::ThinkingAdaptive {
        type_: claude::ThinkingAdaptiveType::Adaptive,
        display: None,
        extra: Default::default(),
    }))
}

fn gemini_thinking_to_claude_output_effort(
    thinking: &gemini::ThinkingConfig,
) -> Option<claude::OutputEffort> {
    let effort = match thinking.thinking_level.as_ref()? {
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Minimal)
        | gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Low) => {
            claude::OutputEffortKnown::Low
        }
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::Medium)
        | gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::ThinkingLevelUnspecified) => {
            claude::OutputEffortKnown::Medium
        }
        gemini::ThinkingLevel::Known(gemini::ThinkingLevelKnown::High)
        | gemini::ThinkingLevel::Unknown(_) => claude::OutputEffortKnown::High,
    };
    Some(claude::OutputEffort::Known(effort))
}

pub(in crate::transform::count_tokens) fn gemini_service_tier_to_claude(
    service_tier: Option<gemini::ServiceTier>,
) -> Option<claude::RequestServiceTier> {
    let service_tier = match service_tier? {
        gemini::ServiceTier::Known(gemini::ServiceTierKnown::Standard) => {
            claude::RequestServiceTierKnown::StandardOnly
        }
        gemini::ServiceTier::Known(gemini::ServiceTierKnown::Flex)
        | gemini::ServiceTier::Known(gemini::ServiceTierKnown::Priority)
        | gemini::ServiceTier::Known(gemini::ServiceTierKnown::Unspecified) => {
            claude::RequestServiceTierKnown::Auto
        }
        gemini::ServiceTier::Unknown(_) => claude::RequestServiceTierKnown::StandardOnly,
    };
    Some(claude::RequestServiceTier::Known(service_tier))
}

pub(in crate::transform::count_tokens) fn openai_service_tier_to_claude(
    service_tier: Option<openai::ServiceTier>,
) -> Option<claude::RequestServiceTier> {
    let service_tier = match service_tier? {
        openai::ServiceTier::Auto => claude::RequestServiceTierKnown::Auto,
        openai::ServiceTier::Default => claude::RequestServiceTierKnown::StandardOnly,
        openai::ServiceTier::Flex | openai::ServiceTier::Scale | openai::ServiceTier::Priority => {
            claude::RequestServiceTierKnown::Auto
        }
    };
    Some(claude::RequestServiceTier::Known(service_tier))
}

pub(in crate::transform::count_tokens) fn gemini_service_tier_to_openai(
    service_tier: Option<gemini::ServiceTier>,
) -> Option<openai::ServiceTier> {
    let service_tier = match service_tier? {
        gemini::ServiceTier::Known(gemini::ServiceTierKnown::Flex) => openai::ServiceTier::Flex,
        gemini::ServiceTier::Known(gemini::ServiceTierKnown::Priority) => {
            openai::ServiceTier::Priority
        }
        gemini::ServiceTier::Known(gemini::ServiceTierKnown::Standard)
        | gemini::ServiceTier::Known(gemini::ServiceTierKnown::Unspecified)
        | gemini::ServiceTier::Unknown(_) => openai::ServiceTier::Default,
    };
    Some(service_tier)
}

pub(in crate::transform::count_tokens) fn claude_service_tier_to_openai(
    service_tier: Option<claude::RequestServiceTier>,
) -> Option<openai::ServiceTier> {
    let service_tier = match service_tier? {
        claude::RequestServiceTier::Known(claude::RequestServiceTierKnown::Auto) => {
            openai::ServiceTier::Auto
        }
        claude::RequestServiceTier::Known(claude::RequestServiceTierKnown::StandardOnly)
        | claude::RequestServiceTier::Unknown(_) => openai::ServiceTier::Default,
    };
    Some(service_tier)
}

pub(in crate::transform::count_tokens) fn openai_reasoning_to_claude(
    reasoning: Option<openai::ReasoningConfig>,
) -> Option<claude::ThinkingConfig> {
    match reasoning?.effort? {
        openai::ReasoningEffort::None => {
            Some(claude::ThinkingConfig::Disabled(claude::ThinkingDisabled {
                type_: claude::ThinkingDisabledType::Disabled,
                extra: Default::default(),
            }))
        }
        _ => Some(claude::ThinkingConfig::Adaptive(claude::ThinkingAdaptive {
            type_: claude::ThinkingAdaptiveType::Adaptive,
            display: None,
            extra: Default::default(),
        })),
    }
}

pub(in crate::transform::count_tokens) fn openai_text_to_claude_output_format(
    text: Option<openai::TextConfig>,
) -> Option<claude::JsonSchemaFormat> {
    openai_response_format_to_claude(&text?.format?)
}

pub(in crate::transform::count_tokens) fn openai_generation_to_claude_output_config(
    reasoning: Option<&openai::ReasoningConfig>,
    text: Option<&openai::TextConfig>,
) -> Option<claude::OutputConfig> {
    let effort = reasoning
        .and_then(|reasoning| reasoning.effort.as_ref())
        .map(openai_reasoning_effort_to_claude_output);
    let format = text
        .and_then(|text| text.format.as_ref())
        .and_then(openai_response_format_to_claude);

    if effort.is_none() && format.is_none() {
        return None;
    }

    Some(claude::OutputConfig {
        effort,
        format,
        task_budget: None,
        extra: Default::default(),
    })
}

fn openai_reasoning_effort_to_claude_output(
    effort: &openai::ReasoningEffort,
) -> claude::OutputEffort {
    let effort = match effort {
        openai::ReasoningEffort::None
        | openai::ReasoningEffort::Minimal
        | openai::ReasoningEffort::Low => claude::OutputEffortKnown::Low,
        openai::ReasoningEffort::Medium => claude::OutputEffortKnown::Medium,
        openai::ReasoningEffort::High => claude::OutputEffortKnown::High,
        openai::ReasoningEffort::XHigh => claude::OutputEffortKnown::XHigh,
    };
    claude::OutputEffort::Known(effort)
}

fn openai_response_format_to_claude(
    format: &openai::ResponseFormat,
) -> Option<claude::JsonSchemaFormat> {
    let openai::ResponseFormat::JsonSchema(format) = format else {
        return None;
    };
    Some(claude::JsonSchemaFormat {
        type_: claude::JsonSchemaFormatType::Known(claude::JsonSchemaFormatTypeKnown::JsonSchema),
        schema: format.schema.clone(),
        extra: Default::default(),
    })
}
