use crate::gemini::embeddings::response::GeminiEmbedContentResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiEmbedContentResponse> for GeminiEmbedContentResponse {
    type Error = TransformError;

    fn try_from(value: &GeminiEmbedContentResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
