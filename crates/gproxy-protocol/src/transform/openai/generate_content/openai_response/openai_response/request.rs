use crate::openai::create_response::request::OpenAiCreateResponseRequest;
use crate::openai::create_response::types::HttpMethod as OpenAiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiCreateResponseRequest> for OpenAiCreateResponseRequest {
    type Error = TransformError;

    fn try_from(value: &OpenAiCreateResponseRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = OpenAiHttpMethod::Post;
        Ok(output)
    }
}
