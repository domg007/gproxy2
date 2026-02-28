use crate::gemini::generate_content::request::GeminiGenerateContentRequest;
use crate::gemini::generate_content::types::HttpMethod as GeminiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiGenerateContentRequest> for GeminiGenerateContentRequest {
    type Error = TransformError;

    fn try_from(value: &GeminiGenerateContentRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = GeminiHttpMethod::Post;
        Ok(output)
    }
}
