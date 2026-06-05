//! Claude -> OpenAI model transforms.

use crate::protocol::{claude, openai};
use crate::transform::{TransformContext, TransformError};

use super::{DEFAULT_OPENAI_OWNED_BY, openai_model_object, wire_string};

pub fn list_request(_: claude::ListModelsQuery, _: &TransformContext) {}

pub fn list_response(
    input: claude::ListModelsResponse,
    ctx: &TransformContext,
) -> Result<openai::ModelListResponse, TransformError> {
    Ok(openai::ModelListResponse {
        data: input
            .data
            .into_iter()
            .map(|model| model_response(model, ctx))
            .collect::<Result<Vec<_>, _>>()?,
        object: openai::ListObjectType::List,
        extra: Default::default(),
    })
}

pub fn get_response(
    input: claude::ModelInfo,
    ctx: &TransformContext,
) -> Result<openai::Model, TransformError> {
    model_response(input, ctx)
}

fn model_response(
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
