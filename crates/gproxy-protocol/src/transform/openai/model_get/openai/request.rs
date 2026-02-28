use crate::openai::model_get::request::OpenAiModelGetRequest;
use crate::openai::types::HttpMethod as OpenAiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiModelGetRequest> for OpenAiModelGetRequest {
    type Error = TransformError;

    fn try_from(value: &OpenAiModelGetRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = OpenAiHttpMethod::Get;
        Ok(output)
    }
}
