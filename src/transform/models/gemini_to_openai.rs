//! Gemini -> OpenAI model transforms.

use crate::protocol::{gemini, openai};
use crate::transform::{TransformContext, TransformError};

use super::{DEFAULT_OPENAI_OWNED_BY, openai_model_id, openai_model_object};

pub fn list_request(_: gemini::ListModelsRequest, _: &TransformContext) {}

pub fn list_response(
    input: gemini::ListModelsResponse,
    ctx: &TransformContext,
) -> Result<openai::ModelListResponse, TransformError> {
    Ok(openai::ModelListResponse {
        data: input
            .models
            .into_iter()
            .map(|model| model_response(model, ctx))
            .collect(),
        object: openai::ListObjectType::List,
        extra: Default::default(),
    })
}

pub fn get_response(input: gemini::Model, ctx: &TransformContext) -> openai::Model {
    model_response(input, ctx)
}

fn model_response(input: gemini::Model, _: &TransformContext) -> openai::Model {
    openai::Model {
        id: openai_model_id(gemini_model_id(&input)),
        created: 0,
        object: openai_model_object(),
        owned_by: DEFAULT_OPENAI_OWNED_BY.to_owned(),
        extra: Default::default(),
    }
}

fn gemini_model_id(input: &gemini::Model) -> String {
    input
        .base_model_id
        .clone()
        .or_else(|| input.name.clone())
        .unwrap_or_default()
}
