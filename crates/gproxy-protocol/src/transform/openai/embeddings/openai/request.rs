use crate::openai::embeddings::request::OpenAiEmbeddingsRequest;
use crate::openai::embeddings::types::HttpMethod as OpenAiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiEmbeddingsRequest> for OpenAiEmbeddingsRequest {
    type Error = TransformError;

    fn try_from(value: &OpenAiEmbeddingsRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = OpenAiHttpMethod::Post;
        Ok(output)
    }
}
