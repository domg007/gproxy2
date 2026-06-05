//! Claude -> Gemini model transforms.

use crate::protocol::{claude, gemini};
use crate::transform::{TransformContext, TransformError};

use super::{u64_to_i32_default, wire_string};

pub fn list_request(
    input: claude::ListModelsQuery,
    _: &TransformContext,
) -> Result<gemini::ListModelsRequest, TransformError> {
    Ok(gemini::ListModelsRequest {
        page_size: input.limit.map(u64_to_i32_default),
        page_token: input.after_id,
        extra: Default::default(),
    })
}

pub fn get_request(
    input: claude::RetrieveModelPath,
    _: &TransformContext,
) -> Result<gemini::GetModelRequest, TransformError> {
    Ok(gemini::GetModelRequest {
        name: Some(wire_string(&input.model_id, "model_id")?),
        extra: Default::default(),
    })
}

pub fn list_response(
    input: claude::ListModelsResponse,
    ctx: &TransformContext,
) -> Result<gemini::ListModelsResponse, TransformError> {
    Ok(gemini::ListModelsResponse {
        models: input
            .data
            .into_iter()
            .map(|model| model_response(model, ctx))
            .collect::<Result<Vec<_>, _>>()?,
        next_page_token: if input.has_more {
            Some(input.last_id)
        } else {
            None
        },
        extra: Default::default(),
    })
}

pub fn get_response(
    input: claude::ModelInfo,
    ctx: &TransformContext,
) -> Result<gemini::Model, TransformError> {
    model_response(input, ctx)
}

fn model_response(
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
