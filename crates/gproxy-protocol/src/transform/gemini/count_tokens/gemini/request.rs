use crate::gemini::count_tokens::request::GeminiCountTokensRequest;
use crate::gemini::types::HttpMethod as GeminiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiCountTokensRequest> for GeminiCountTokensRequest {
    type Error = TransformError;

    fn try_from(value: &GeminiCountTokensRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = GeminiHttpMethod::Post;
        Ok(output)
    }
}
