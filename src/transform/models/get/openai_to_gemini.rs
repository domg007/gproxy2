//! OpenAI -> Gemini get-model transforms.

use crate::protocol::{gemini, openai};
use crate::transform::{TransformContext, TransformError};

use super::super::model;

pub fn response(
    input: openai::Model,
    ctx: &TransformContext,
) -> Result<gemini::Model, TransformError> {
    model::openai_to_gemini(input, ctx)
}
