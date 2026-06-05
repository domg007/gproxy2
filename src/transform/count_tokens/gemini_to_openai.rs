//! Gemini -> OpenAI count-token transforms.

use crate::protocol::{gemini, openai};
use crate::transform::{TransformContext, TransformError};

use super::common;

pub fn request(
    input: gemini::CountTokensRequest,
    _: &TransformContext,
) -> Result<openai::ResponseInputTokensRequest, TransformError> {
    let request = split_gemini_request(input);

    Ok(openai::ResponseInputTokensRequest {
        conversation: None,
        input: common::text_to_openai_input(common::gemini_contents_to_text(request.contents)),
        instructions: request.system_instruction.map(common::gemini_content_text),
        model: Some(common::gemini_model_string(request.model).into()),
        parallel_tool_calls: None,
        personality: None,
        previous_response_id: None,
        reasoning: common::gemini_generation_to_openai_reasoning(
            request.generation_config.as_ref(),
        ),
        text: common::gemini_generation_to_openai_text(request.generation_config.as_ref()),
        tool_choice: common::gemini_tool_config_to_openai(request.tool_config),
        tools: common::gemini_tools_to_openai(request.tools),
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

struct GeminiCountTokenParts {
    model: Option<String>,
    contents: Vec<gemini::Content>,
    system_instruction: Option<gemini::Content>,
    tools: Vec<gemini::Tool>,
    tool_config: Option<gemini::ToolConfig>,
    generation_config: Option<gemini::GenerationConfig>,
}

fn split_gemini_request(input: gemini::CountTokensRequest) -> GeminiCountTokenParts {
    let mut model = input.model;
    let mut contents = input.contents;
    let mut system_instruction = None;
    let mut tools = Vec::new();
    let mut tool_config = None;
    let mut generation_config = None;

    if let Some(request) = input.generate_content_request {
        if model.is_none() {
            model = request.model;
        }
        if contents.is_empty() {
            contents = request.contents;
        }
        system_instruction = request.system_instruction;
        tools = request.tools;
        tool_config = request.tool_config;
        generation_config = request.generation_config;
    }

    GeminiCountTokenParts {
        model,
        contents,
        system_instruction,
        tools,
        tool_config,
        generation_config,
    }
}
