use crate::gemini::embeddings::request::GeminiEmbedContentRequest;
use crate::gemini::types::HttpMethod as GeminiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiEmbedContentRequest> for GeminiEmbedContentRequest {
    type Error = TransformError;

    fn try_from(value: &GeminiEmbedContentRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = GeminiHttpMethod::Post;
        Ok(output)
    }
}
