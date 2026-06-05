//! Claude -> Gemini count-token transforms.

use crate::protocol::{claude, gemini};
use crate::transform::{TransformContext, TransformError};

use super::common;

pub fn request(
    input: claude::CountTokensRequestBody,
    _: &TransformContext,
) -> Result<gemini::CountTokensRequest, TransformError> {
    let system_instruction = common::claude_system_to_text(input.system).map(|text| {
        common::text_to_gemini_content(
            text,
            Some(gemini::ContentRole::Known(gemini::ContentRoleKnown::System)),
        )
    });

    Ok(gemini::CountTokensRequest {
        model: Some(common::claude_model_string(&input.model)),
        contents: common::text_to_gemini_contents(common::claude_messages_to_text(input.messages)),
        generate_content_request: Some(gemini::GenerateContentRequest {
            model: Some(common::claude_model_string(&input.model)),
            contents: Vec::new(),
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
    input: claude::CountTokensResponseBody,
    _: &TransformContext,
) -> gemini::CountTokensResponse {
    gemini::CountTokensResponse {
        total_tokens: Some(common::u64_to_i32(input.input_tokens)),
        cached_content_token_count: input
            .context_management
            .and_then(|context| context.original_input_tokens)
            .map(common::u64_to_i32),
        prompt_tokens_details: Vec::new(),
        cache_tokens_details: Vec::new(),
        extra: Default::default(),
    }
}
