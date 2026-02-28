use crate::gemini::stream_generate_content::response::GeminiStreamGenerateContentResponse;
use crate::openai::create_chat_completions::stream::OpenAiChatCompletionsSseStreamBody;
use crate::openai::create_response::stream::OpenAiCreateResponseSseStreamBody;
use crate::transform::utils::TransformError;

impl TryFrom<GeminiStreamGenerateContentResponse> for OpenAiChatCompletionsSseStreamBody {
    type Error = TransformError;

    fn try_from(value: GeminiStreamGenerateContentResponse) -> Result<Self, TransformError> {
        let response_stream = OpenAiCreateResponseSseStreamBody::try_from(value)?;
        OpenAiChatCompletionsSseStreamBody::try_from(response_stream)
    }
}
