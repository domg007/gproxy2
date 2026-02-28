use crate::openai::count_tokens::request::OpenAiCountTokensRequest;
use crate::openai::count_tokens::types::HttpMethod as OpenAiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiCountTokensRequest> for OpenAiCountTokensRequest {
    type Error = TransformError;

    fn try_from(value: &OpenAiCountTokensRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = OpenAiHttpMethod::Post;
        Ok(output)
    }
}
