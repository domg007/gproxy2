//! Claude -> OpenAI count-token transforms.

use crate::protocol::{claude, openai};
use crate::transform::{TransformContext, TransformError};

use super::common;

pub fn request(
    input: claude::CountTokensRequestBody,
    _: &TransformContext,
) -> Result<openai::ResponseInputTokensRequest, TransformError> {
    Ok(openai::ResponseInputTokensRequest {
        conversation: None,
        input: common::text_to_openai_input(common::claude_messages_to_text(input.messages)),
        instructions: common::claude_system_to_text(input.system),
        model: Some(common::claude_model_string(&input.model).into()),
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
    input: claude::CountTokensResponseBody,
    _: &TransformContext,
) -> openai::ResponseInputTokensResponse {
    openai::ResponseInputTokensResponse {
        input_tokens: common::u64_to_u32(input.input_tokens),
        object: openai::ResponseInputTokensObjectType::ResponseInputTokens,
        extra: Default::default(),
    }
}
