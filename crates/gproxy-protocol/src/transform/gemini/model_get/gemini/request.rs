use crate::gemini::model_get::request::GeminiModelGetRequest;
use crate::gemini::types::HttpMethod as GeminiHttpMethod;
use crate::transform::utils::TransformError;

impl TryFrom<&GeminiModelGetRequest> for GeminiModelGetRequest {
    type Error = TransformError;

    fn try_from(value: &GeminiModelGetRequest) -> Result<Self, TransformError> {
        let mut output = value.clone();
        output.method = GeminiHttpMethod::Get;
        Ok(output)
    }
}
