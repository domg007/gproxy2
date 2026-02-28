use crate::gemini::count_tokens::response::GeminiCountTokensResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiCountTokensResponse> for GeminiCountTokensResponse {
    type Error = TransformError;

    fn try_from(value: &GeminiCountTokensResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
