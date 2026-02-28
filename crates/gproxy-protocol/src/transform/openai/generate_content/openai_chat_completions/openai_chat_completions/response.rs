use crate::openai::create_chat_completions::response::OpenAiChatCompletionsResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiChatCompletionsResponse> for OpenAiChatCompletionsResponse {
    type Error = TransformError;

    fn try_from(value: &OpenAiChatCompletionsResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
