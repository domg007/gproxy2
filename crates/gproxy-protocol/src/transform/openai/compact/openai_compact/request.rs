use crate::openai::compact_response::request::OpenAiCompactRequest;
use crate::openai::compact_response::types::HttpMethod as OpenAiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiCompactRequest> for OpenAiCompactRequest {
    type Error = TransformError;

    fn try_from(value: &OpenAiCompactRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = OpenAiHttpMethod::Post;
        Ok(output)
    }
}
