//! OpenAI -> Claude get-model transforms.

use crate::protocol::{claude, openai};
use crate::transform::{TransformContext, TransformError};

use super::super::model;

pub fn response(
    input: openai::Model,
    ctx: &TransformContext,
) -> Result<claude::ModelInfo, TransformError> {
    model::openai_to_claude(input, ctx)
}
