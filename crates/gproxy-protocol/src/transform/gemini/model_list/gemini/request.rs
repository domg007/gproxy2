use crate::gemini::model_list::request::GeminiModelListRequest;
use crate::gemini::types::HttpMethod as GeminiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiModelListRequest> for GeminiModelListRequest {
    type Error = TransformError;

    fn try_from(value: &GeminiModelListRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = GeminiHttpMethod::Get;
        Ok(output)
    }
}
