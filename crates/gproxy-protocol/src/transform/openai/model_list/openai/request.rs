use crate::openai::model_list::request::OpenAiModelListRequest;
use crate::openai::types::HttpMethod as OpenAiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&OpenAiModelListRequest> for OpenAiModelListRequest {
    type Error = TransformError;

    fn try_from(value: &OpenAiModelListRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = OpenAiHttpMethod::Get;
        Ok(output)
    }
}
