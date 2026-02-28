use crate::gemini::model_list::response::GeminiModelListResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiModelListResponse> for GeminiModelListResponse {
    type Error = TransformError;

    fn try_from(value: &GeminiModelListResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
