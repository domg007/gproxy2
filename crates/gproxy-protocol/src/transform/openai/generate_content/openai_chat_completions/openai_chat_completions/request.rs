use crate::openai::create_chat_completions::request::OpenAiChatCompletionsRequest;
use crate::openai::create_chat_completions::types::HttpMethod as OpenAiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiChatCompletionsRequest> for OpenAiChatCompletionsRequest {
    type Error = TransformError;

    fn try_from(value: &OpenAiChatCompletionsRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = OpenAiHttpMethod::Post;
        Ok(output)
    }
}
