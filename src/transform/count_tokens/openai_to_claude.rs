//! OpenAI -> Claude count-token transforms.

use crate::protocol::{claude, openai};
use crate::transform::{TransformContext, TransformError};

use super::common;

pub fn request(
    input: openai::ResponseInputTokensRequest,
    _: &TransformContext,
) -> Result<claude::CountTokensRequestBody, TransformError> {
    Ok(claude::CountTokensRequestBody {
        model: common::openai_model_string(input.model).into(),
        messages: common::text_to_claude_messages(common::openai_input_to_text(input.input)),
        cache_control: None,
        context_management: None,
        mcp_servers: None,
        output_config: None,
        output_format: None,
        speed: None,
        system: common::text_to_claude_system(input.instructions),
        thinking: None,
        tool_choice: None,
        tools: None,
        extra: Default::default(),
    })
}

pub fn response(
    input: openai::ResponseInputTokensResponse,
    _: &TransformContext,
) -> claude::CountTokensResponseBody {
    claude::CountTokensResponseBody {
        input_tokens: common::u32_to_u64(input.input_tokens),
        context_management: None,
        extra: Default::default(),
    }
}
