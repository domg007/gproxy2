//! Claude -> Gemini count-token transforms.

use crate::protocol::{claude, gemini};
use crate::transform::{TransformContext, TransformError};

use super::common;

pub fn request(
    input: claude::CountTokensRequestBody,
    _: &TransformContext,
) -> Result<gemini::CountTokensRequest, TransformError> {
    let model = common::claude_model_string(&input.model);
    let contents = common::text_to_gemini_contents(common::claude_messages_to_text(input.messages));
    let system_instruction = common::claude_system_to_text(input.system).map(|text| {
        common::text_to_gemini_content(
            text,
            Some(gemini::ContentRole::Known(gemini::ContentRoleKnown::System)),
        )
    });
    let generate_content_request =
        system_instruction.map(|system_instruction| gemini::GenerateContentRequest {
            model: Some(model.clone()),
            contents: contents.clone(),
            tools: Vec::new(),
            tool_config: None,
            safety_settings: Vec::new(),
            system_instruction: Some(system_instruction),
            generation_config: None,
            cached_content: None,
            service_tier: None,
            store: None,
            extra: Default::default(),
        });

    Ok(gemini::CountTokensRequest {
        model: Some(model),
        contents: if generate_content_request.is_some() {
            Vec::new()
        } else {
            contents
        },
        generate_content_request,
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
