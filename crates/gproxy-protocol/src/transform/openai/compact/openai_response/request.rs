use crate::openai::compact_response::request::OpenAiCompactRequest;
use crate::openai::create_response::request::{
    OpenAiCreateResponseRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::openai::create_response::types::HttpMethod as OpenAiHttpMethod;
use crate::transform::openai::compact::utils::{
    COMPACT_MAX_OUTPUT_TOKENS, compact_system_instruction,
};
use crate::transform::utils::TransformError;

impl TryFrom<OpenAiCompactRequest> for OpenAiCreateResponseRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCompactRequest) -> Result<Self, TransformError> {
        let body = value.body;

        Ok(OpenAiCreateResponseRequest {
            method: OpenAiHttpMethod::Post,
            path: PathParameters::default(),
            query: QueryParameters::default(),
            headers: RequestHeaders::default(),
            body: RequestBody {
                input: body.input,
                instructions: Some(compact_system_instruction(body.instructions)),
                max_output_tokens: Some(COMPACT_MAX_OUTPUT_TOKENS),
                model: Some(body.model),
                previous_response_id: body.previous_response_id,
                ..RequestBody::default()
            },
        })
    }
}
