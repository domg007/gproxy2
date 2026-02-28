use crate::gemini::model_get::response::GeminiModelGetResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiModelGetResponse> for GeminiModelGetResponse {
    type Error = TransformError;

    fn try_from(value: &GeminiModelGetResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
