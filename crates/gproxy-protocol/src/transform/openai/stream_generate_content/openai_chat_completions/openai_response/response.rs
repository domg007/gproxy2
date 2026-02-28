use crate::openai::create_chat_completions::response::OpenAiChatCompletionsResponse;
use crate::openai::create_chat_completions::stream::OpenAiChatCompletionsSseStreamBody;
use crate::openai::create_response::response::OpenAiCreateResponseResponse;
use crate::openai::create_response::stream::OpenAiCreateResponseSseStreamBody;
use crate::transform::utils::TransformError;

impl TryFrom<OpenAiCreateResponseSseStreamBody> for OpenAiChatCompletionsSseStreamBody {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseSseStreamBody) -> Result<Self, TransformError> {
        let response = OpenAiCreateResponseResponse::try_from(value)?;
        let chat = OpenAiChatCompletionsResponse::try_from(response)?;
        OpenAiChatCompletionsSseStreamBody::try_from(chat)
    }
}
