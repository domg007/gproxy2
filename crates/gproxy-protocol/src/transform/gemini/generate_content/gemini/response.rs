use crate::gemini::generate_content::response::GeminiGenerateContentResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiGenerateContentResponse> for GeminiGenerateContentResponse {
    type Error = TransformError;

    fn try_from(value: &GeminiGenerateContentResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
