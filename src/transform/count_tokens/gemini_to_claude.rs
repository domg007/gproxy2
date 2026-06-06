//! Gemini -> Claude count-token transforms.

use crate::protocol::{claude, gemini};
use crate::transform::{TransformContext, TransformError};

use super::common;

pub fn request(
    input: gemini::CountTokensRequest,
    _: &TransformContext,
) -> Result<claude::CountTokensRequestBody, TransformError> {
    let (model, contents, system_instruction) = split_gemini_request(input);

    Ok(claude::CountTokensRequestBody {
        model: common::gemini_model_string(model).into(),
        messages: common::text_to_claude_messages(common::gemini_contents_to_text(contents)),
        cache_control: None,
        context_management: None,
        diagnostics: None,
        mcp_servers: None,
        output_config: None,
        output_format: None,
        speed: None,
        system: common::text_to_claude_system(system_instruction.map(common::gemini_content_text)),
        thinking: None,
        tool_choice: None,
        tools: None,
        extra: Default::default(),
    })
}

pub fn response(
    input: gemini::CountTokensResponse,
    _: &TransformContext,
) -> claude::CountTokensResponseBody {
    claude::CountTokensResponseBody {
        input_tokens: input
            .total_tokens
            .map(common::i32_to_u64)
            .unwrap_or_default(),
        context_management: input.cached_content_token_count.map(|cached| {
            claude::CountTokensContextManagement {
                original_input_tokens: Some(common::i32_to_u64(cached)),
                extra: Default::default(),
            }
        }),
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
