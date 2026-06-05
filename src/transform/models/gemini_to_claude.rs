//! Gemini -> Claude model transforms.

use crate::protocol::{claude, gemini};
use crate::transform::{TransformContext, TransformError};

use super::{
    DEFAULT_CREATED_AT, claude_model_id, claude_model_object, default_claude_capabilities,
    i32_to_u64_default,
};

pub fn list_request(
    input: gemini::ListModelsRequest,
    _: &TransformContext,
) -> claude::ListModelsQuery {
    claude::ListModelsQuery {
        after_id: input.page_token,
        before_id: None,
        limit: input.page_size.map(i32_to_u64_default),
        extra: Default::default(),
    }
}

pub fn get_request(
    input: gemini::GetModelRequest,
    _: &TransformContext,
) -> claude::RetrieveModelPath {
    claude::RetrieveModelPath {
        model_id: claude_model_id(input.name.unwrap_or_default()),
    }
}

pub fn list_response(
    input: gemini::ListModelsResponse,
    ctx: &TransformContext,
) -> Result<claude::ListModelsResponse, TransformError> {
    let has_more = input.next_page_token.is_some();
    let data = input
        .models
        .into_iter()
        .map(|model| model_response(model, ctx))
        .collect::<Vec<_>>();

    let first_id = data.first().map(model_id).unwrap_or_default();
    let last_id = input
        .next_page_token
        .or_else(|| data.last().map(model_id))
        .unwrap_or_default();

    Ok(claude::ListModelsResponse {
        data,
        first_id,
        has_more,
        last_id,
        extra: Default::default(),
    })
}

pub fn get_response(input: gemini::Model, ctx: &TransformContext) -> claude::ModelInfo {
    model_response(input, ctx)
}

fn model_response(input: gemini::Model, _: &TransformContext) -> claude::ModelInfo {
    let id = gemini_model_id(&input);

    claude::ModelInfo {
        id: claude_model_id(id.clone()),
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

fn model_id(input: &claude::ModelInfo) -> String {
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
