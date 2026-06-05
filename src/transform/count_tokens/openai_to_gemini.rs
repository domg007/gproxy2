//! OpenAI -> Gemini count-token transforms.

use crate::protocol::{gemini, openai};
use crate::transform::{TransformContext, TransformError};

use super::common;

pub fn request(
    input: openai::ResponseInputTokensRequest,
    _: &TransformContext,
) -> Result<gemini::CountTokensRequest, TransformError> {
    Ok(gemini::CountTokensRequest {
        model: Some(common::openai_model_string(input.model)),
        contents: common::text_to_gemini_contents(common::openai_input_to_text(input.input)),
        generate_content_request: None,
        extra: Default::default(),
    })
}

pub fn response(
    input: openai::ResponseInputTokensResponse,
    _: &TransformContext,
) -> gemini::CountTokensResponse {
    gemini::CountTokensResponse {
        total_tokens: Some(common::u32_to_i32(input.input_tokens)),
        cached_content_token_count: None,
        prompt_tokens_details: Vec::new(),
        cache_tokens_details: Vec::new(),
        extra: Default::default(),
    }
}
