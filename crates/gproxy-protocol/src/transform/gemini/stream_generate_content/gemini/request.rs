use crate::gemini::stream_generate_content::request::GeminiStreamGenerateContentRequest;
use crate::gemini::types::HttpMethod as GeminiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiStreamGenerateContentRequest> for GeminiStreamGenerateContentRequest {
    type Error = TransformError;

    fn try_from(value: &GeminiStreamGenerateContentRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = GeminiHttpMethod::Post;
        Ok(output)
    }
}
