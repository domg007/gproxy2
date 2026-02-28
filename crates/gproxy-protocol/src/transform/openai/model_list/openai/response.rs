use crate::openai::model_list::response::OpenAiModelListResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiModelListResponse> for OpenAiModelListResponse {
    type Error = TransformError;

    fn try_from(value: &OpenAiModelListResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
