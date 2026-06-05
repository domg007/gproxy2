//! OpenAI -> Gemini count-token transforms.

use crate::protocol::{gemini, openai};
use crate::transform::{TransformContext, TransformError};

use super::common;

pub fn request(
    input: openai::ResponseInputTokensRequest,
    _: &TransformContext,
) -> Result<gemini::CountTokensRequest, TransformError> {
    let model = common::openai_model_string(input.model);
    let contents = common::text_to_gemini_contents(common::openai_input_to_text(input.input));
    let system_instruction = input
        .instructions
        .filter(|text| !text.is_empty())
        .map(|text| {
            common::text_to_gemini_content(
                text,
                Some(gemini::ContentRole::Known(gemini::ContentRoleKnown::System)),
            )
        });

    Ok(gemini::CountTokensRequest {
        model: Some(model.clone()),
        contents: Vec::new(),
        generate_content_request: Some(gemini::GenerateContentRequest {
            model: Some(model),
            contents,
            tools: Vec::new(),
            tool_config: None,
            safety_settings: Vec::new(),
            system_instruction,
            generation_config: None,
            cached_content: None,
            service_tier: None,
            store: None,
            extra: Default::default(),
        }),
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
