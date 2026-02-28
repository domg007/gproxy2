use crate::openai::embeddings::response::OpenAiEmbeddingsResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiEmbeddingsResponse> for OpenAiEmbeddingsResponse {
    type Error = TransformError;

    fn try_from(value: &OpenAiEmbeddingsResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
