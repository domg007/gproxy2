//! Gemini -> OpenAI get-model transforms.

use crate::protocol::{gemini, openai};
use crate::transform::TransformContext;

use super::super::model;

pub fn response(input: gemini::Model, ctx: &TransformContext) -> openai::Model {
    model::gemini_to_openai(input, ctx)
}
