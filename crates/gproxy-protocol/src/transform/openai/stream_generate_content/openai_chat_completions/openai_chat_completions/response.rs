use crate::openai::create_chat_completions::stream::OpenAiChatCompletionsSseStreamBody;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiChatCompletionsSseStreamBody> for OpenAiChatCompletionsSseStreamBody {
    type Error = TransformError;

    fn try_from(value: &OpenAiChatCompletionsSseStreamBody) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
