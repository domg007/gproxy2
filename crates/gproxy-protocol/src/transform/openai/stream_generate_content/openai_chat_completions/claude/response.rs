use crate::claude::create_message::stream::ClaudeCreateMessageSseStreamBody;
use crate::openai::create_chat_completions::stream::OpenAiChatCompletionsSseStreamBody;
use crate::openai::create_response::stream::OpenAiCreateResponseSseStreamBody;
use crate::transform::utils::TransformError;

impl TryFrom<ClaudeCreateMessageSseStreamBody> for OpenAiChatCompletionsSseStreamBody {
    type Error = TransformError;

    fn try_from(value: ClaudeCreateMessageSseStreamBody) -> Result<Self, TransformError> {
        let response_stream = OpenAiCreateResponseSseStreamBody::try_from(value)?;
        OpenAiChatCompletionsSseStreamBody::try_from(response_stream)
    }
}
