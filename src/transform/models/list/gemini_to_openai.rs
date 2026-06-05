//! Gemini -> OpenAI list-models transforms.

use crate::protocol::{gemini, openai};
use crate::transform::{TransformContext, TransformError};

use super::super::model;

pub fn request(_: gemini::ListModelsRequest, _: &TransformContext) {}

pub fn response(
    input: gemini::ListModelsResponse,
    ctx: &TransformContext,
) -> Result<openai::ModelListResponse, TransformError> {
    Ok(openai::ModelListResponse {
        data: input
            .models
            .into_iter()
            .map(|model| model::gemini_to_openai(model, ctx))
            .collect(),
        object: openai::ListObjectType::List,
        extra: Default::default(),
    })
}
