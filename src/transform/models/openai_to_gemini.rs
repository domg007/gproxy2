//! OpenAI -> Gemini model transforms.

use crate::protocol::{gemini, openai};
use crate::transform::{TransformContext, TransformError};

use super::wire_string;

pub fn list_request(_: (), _: &TransformContext) -> gemini::ListModelsRequest {
    gemini::ListModelsRequest::default()
}

pub fn list_response(
    input: openai::ModelListResponse,
    ctx: &TransformContext,
) -> Result<gemini::ListModelsResponse, TransformError> {
    Ok(gemini::ListModelsResponse {
        models: input
            .data
            .into_iter()
            .map(|model| model_response(model, ctx))
            .collect::<Result<Vec<_>, _>>()?,
        next_page_token: None,
        extra: Default::default(),
    })
}

pub fn get_response(
    input: openai::Model,
    ctx: &TransformContext,
) -> Result<gemini::Model, TransformError> {
    model_response(input, ctx)
}

fn model_response(
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
