use crate::openai::count_tokens::response::OpenAiCountTokensResponse;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiCountTokensResponse> for OpenAiCountTokensResponse {
    type Error = TransformError;

    fn try_from(value: &OpenAiCountTokensResponse) -> Result<Self, TransformError> {
        Ok(value.clone())
    }
}
