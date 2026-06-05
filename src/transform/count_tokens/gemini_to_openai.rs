//! Gemini -> OpenAI count-token transforms.

use crate::protocol::{gemini, openai};
use crate::transform::{TransformContext, TransformError};

use super::common;

pub fn request(
    input: gemini::CountTokensRequest,
    _: &TransformContext,
) -> Result<openai::ResponseInputTokensRequest, TransformError> {
    let (model, contents, system_instruction) = split_gemini_request(input);

    Ok(openai::ResponseInputTokensRequest {
        conversation: None,
        input: common::text_to_openai_input(common::gemini_contents_to_text(contents)),
        instructions: system_instruction.map(common::gemini_content_text),
        model: Some(common::gemini_model_string(model).into()),
        parallel_tool_calls: None,
        personality: None,
        previous_response_id: None,
        reasoning: None,
        text: None,
        tool_choice: None,
        tools: None,
        truncation: None,
        extra: Default::default(),
    })
}

pub fn response(
    input: gemini::CountTokensResponse,
    _: &TransformContext,
) -> openai::ResponseInputTokensResponse {
    openai::ResponseInputTokensResponse {
        input_tokens: input
            .total_tokens
            .map(common::i32_to_u32)
            .unwrap_or_default(),
        object: openai::ResponseInputTokensObjectType::ResponseInputTokens,
        extra: Default::default(),
    }
}

fn split_gemini_request(
    input: gemini::CountTokensRequest,
) -> (
    Option<String>,
    Vec<gemini::Content>,
    Option<gemini::Content>,
) {
    let mut model = input.model;
    let mut contents = input.contents;
    let mut system_instruction = None;

    if let Some(request) = input.generate_content_request {
        if model.is_none() {
            model = request.model;
        }
        if contents.is_empty() {
            contents = request.contents;
        }
        system_instruction = request.system_instruction;
    }

    (model, contents, system_instruction)
}
