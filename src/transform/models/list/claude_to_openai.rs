//! Claude -> OpenAI list-models transforms.

use crate::protocol::{claude, openai};
use crate::transform::{TransformContext, TransformError};

use super::super::model;

pub fn request(_: claude::ListModelsQuery, _: &TransformContext) {}

pub fn response(
    input: claude::ListModelsResponse,
    ctx: &TransformContext,
) -> Result<openai::ModelListResponse, TransformError> {
    Ok(openai::ModelListResponse {
        data: input
            .data
            .into_iter()
            .map(|model| model::claude_to_openai(model, ctx))
            .collect::<Result<Vec<_>, _>>()?,
        object: openai::ListObjectType::List,
        extra: Default::default(),
    })
}
