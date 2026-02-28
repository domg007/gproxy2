use crate::gemini::stream_generate_content::response::GeminiStreamGenerateContentResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiStreamGenerateContentResponse> for GeminiStreamGenerateContentResponse {
    type Error = TransformError;

    fn try_from(value: &GeminiStreamGenerateContentResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
