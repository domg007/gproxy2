//! Claude -> Gemini get-model transforms.

use crate::protocol::{claude, gemini};
use crate::transform::{TransformContext, TransformError};

use super::super::{model, wire_string};

pub fn request(
    input: claude::RetrieveModelPath,
    _: &TransformContext,
) -> Result<gemini::GetModelRequest, TransformError> {
    Ok(gemini::GetModelRequest {
        name: Some(wire_string(&input.model_id, "model_id")?),
        extra: Default::default(),
    })
}

pub fn response(
    input: claude::ModelInfo,
    ctx: &TransformContext,
) -> Result<gemini::Model, TransformError> {
    model::claude_to_gemini(input, ctx)
}
