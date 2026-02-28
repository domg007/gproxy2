use crate::openai::create_response::response::OpenAiCreateResponseResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiCreateResponseResponse> for OpenAiCreateResponseResponse {
    type Error = TransformError;

    fn try_from(value: &OpenAiCreateResponseResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
