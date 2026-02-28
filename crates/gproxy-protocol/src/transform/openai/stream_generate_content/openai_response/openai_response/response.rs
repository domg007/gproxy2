use crate::openai::create_response::stream::OpenAiCreateResponseSseStreamBody;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiCreateResponseSseStreamBody> for OpenAiCreateResponseSseStreamBody {
    type Error = TransformError;

    fn try_from(value: &OpenAiCreateResponseSseStreamBody) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
