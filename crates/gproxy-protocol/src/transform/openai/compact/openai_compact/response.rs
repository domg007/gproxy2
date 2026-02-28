use crate::openai::compact_response::response::OpenAiCompactResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiCompactResponse> for OpenAiCompactResponse {
    type Error = TransformError;

    fn try_from(value: &OpenAiCompactResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
