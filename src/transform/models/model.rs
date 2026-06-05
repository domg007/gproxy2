use crate::protocol::{claude, gemini, openai};
use crate::transform::{TransformContext, TransformError};

use super::{
    DEFAULT_CREATED_AT, DEFAULT_OPENAI_OWNED_BY, claude_model_object, default_claude_capabilities,
    i32_to_u64_default, openai_model_object, u64_to_i32_default, wire_string,
};

pub(in crate::transform::models) fn openai_to_claude(
    input: openai::Model,
    _: &TransformContext,
) -> Result<claude::ModelInfo, TransformError> {
    let id = wire_string(&input.id, "id")?;

    Ok(claude::ModelInfo {
        id: id.clone().into(),
        type_: claude_model_object(),
        created_at: DEFAULT_CREATED_AT.to_owned(),
        display_name: id,
        max_input_tokens: 0,
        max_tokens: 0,
        capabilities: default_claude_capabilities(),
        extra: Default::default(),
    })
}

pub(in crate::transform::models) fn openai_to_gemini(
    input: openai::Model,
    _: &TransformContext,
) -> Result<gemini::Model, TransformError> {
    let id = wire_string(&input.id, "id")?;

    Ok(gemini::Model {
        name: Some(id.clone()),
        base_model_id: Some(id.clone()),
        version: None,
        display_name: Some(id),
        description: None,
        input_token_limit: None,
        output_token_limit: None,
        supported_generation_methods: Vec::new(),
        supported_actions: Vec::new(),
        thinking: None,
        temperature: None,
        max_temperature: None,
        top_p: None,
        top_k: None,
        extra: Default::default(),
    })
}

pub(in crate::transform::models) fn claude_to_openai(
    input: claude::ModelInfo,
    _: &TransformContext,
) -> Result<openai::Model, TransformError> {
    Ok(openai::Model {
        id: wire_string(&input.id, "id")?.into(),
        created: 0,
        object: openai_model_object(),
        owned_by: DEFAULT_OPENAI_OWNED_BY.to_owned(),
        extra: Default::default(),
    })
}

pub(in crate::transform::models) fn claude_to_gemini(
    input: claude::ModelInfo,
    _: &TransformContext,
) -> Result<gemini::Model, TransformError> {
    let id = wire_string(&input.id, "id")?;

    Ok(gemini::Model {
        name: Some(id.clone()),
        base_model_id: Some(id),
        version: None,
        display_name: Some(input.display_name),
        description: None,
        input_token_limit: Some(u64_to_i32_default(input.max_input_tokens)),
        output_token_limit: Some(u64_to_i32_default(input.max_tokens)),
        supported_generation_methods: Vec::new(),
        supported_actions: Vec::new(),
        thinking: None,
        temperature: None,
        max_temperature: None,
        top_p: None,
        top_k: None,
        extra: Default::default(),
    })
}

pub(in crate::transform::models) fn gemini_to_openai(
    input: gemini::Model,
    _: &TransformContext,
) -> openai::Model {
    openai::Model {
        id: gemini_model_id(&input).into(),
        created: 0,
        object: openai_model_object(),
        owned_by: DEFAULT_OPENAI_OWNED_BY.to_owned(),
        extra: Default::default(),
    }
}

pub(in crate::transform::models) fn gemini_to_claude(
    input: gemini::Model,
    _: &TransformContext,
) -> claude::ModelInfo {
    let id = gemini_model_id(&input);

    claude::ModelInfo {
        id: id.clone().into(),
        type_: claude_model_object(),
        created_at: DEFAULT_CREATED_AT.to_owned(),
        display_name: input.display_name.unwrap_or(id),
        max_input_tokens: input
            .input_token_limit
            .map(i32_to_u64_default)
            .unwrap_or_default(),
        max_tokens: input
            .output_token_limit
            .map(i32_to_u64_default)
            .unwrap_or_default(),
        capabilities: default_claude_capabilities(),
        extra: Default::default(),
    }
}

pub(in crate::transform::models) fn claude_model_id(input: &claude::ModelInfo) -> String {
    match &input.id {
        claude::ClaudeModel::Known(known) => serde_json::to_value(known)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default(),
        claude::ClaudeModel::Unknown(value) => value.clone(),
    }
}

fn gemini_model_id(input: &gemini::Model) -> String {
    input
        .base_model_id
        .clone()
        .or_else(|| input.name.clone())
        .unwrap_or_default()
}
