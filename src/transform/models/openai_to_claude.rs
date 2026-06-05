//! OpenAI -> Claude model transforms.

use crate::protocol::{claude, openai};
use crate::transform::{TransformContext, TransformError};

use super::{
    DEFAULT_CREATED_AT, claude_model_id, claude_model_object, default_claude_capabilities,
    wire_string,
};

pub fn list_request(_: (), _: &TransformContext) -> claude::ListModelsQuery {
    claude::ListModelsQuery {
        after_id: None,
        before_id: None,
        limit: None,
        extra: Default::default(),
    }
}

pub fn list_response(
    input: openai::ModelListResponse,
    ctx: &TransformContext,
) -> Result<claude::ListModelsResponse, TransformError> {
    let data = input
        .data
        .into_iter()
        .map(|model| model_response(model, ctx))
        .collect::<Result<Vec<_>, _>>()?;

    let first_id = data
        .first()
        .map(|model| wire_string(&model.id, "id"))
        .transpose()?
        .unwrap_or_default();
    let last_id = data
        .last()
        .map(|model| wire_string(&model.id, "id"))
        .transpose()?
        .unwrap_or_default();

    Ok(claude::ListModelsResponse {
        data,
        first_id,
        has_more: false,
        last_id,
        extra: Default::default(),
    })
}

pub fn get_response(
    input: openai::Model,
    ctx: &TransformContext,
) -> Result<claude::ModelInfo, TransformError> {
    model_response(input, ctx)
}

fn model_response(
    input: openai::Model,
    _: &TransformContext,
) -> Result<claude::ModelInfo, TransformError> {
    let id = wire_string(&input.id, "id")?;

    Ok(claude::ModelInfo {
        id: claude_model_id(id.clone()),
        type_: claude_model_object(),
        created_at: DEFAULT_CREATED_AT.to_owned(),
        display_name: id,
        max_input_tokens: 0,
        max_tokens: 0,
        capabilities: default_claude_capabilities(),
        extra: Default::default(),
    })
}
