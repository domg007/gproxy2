//! Claude -> OpenAI get-model transforms.

use crate::protocol::{claude, openai};
use crate::transform::{TransformContext, TransformError};

use super::super::model;

pub fn response(
    input: claude::ModelInfo,
    ctx: &TransformContext,
) -> Result<openai::Model, TransformError> {
    model::claude_to_openai(input, ctx)
}
