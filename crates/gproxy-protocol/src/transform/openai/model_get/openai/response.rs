use crate::openai::model_get::response::OpenAiModelGetResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiModelGetResponse> for OpenAiModelGetResponse {
    type Error = TransformError;

    fn try_from(value: &OpenAiModelGetResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
