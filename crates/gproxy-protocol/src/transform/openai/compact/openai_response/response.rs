use super::utils::compact_response_body_from_create_response;
use crate::openai::compact_response::response::OpenAiCompactResponse;
use crate::openai::create_response::response::OpenAiCreateResponseResponse;
use crate::transform::utils::TransformError;

impl TryFrom<OpenAiCreateResponseResponse> for OpenAiCompactResponse {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseResponse) -> Result<Self, TransformError> {
        Ok(match value {
            OpenAiCreateResponseResponse::Success {
                stats_code,
                headers,
                body,
            } => OpenAiCompactResponse::Success {
                stats_code,
                headers,
                body: compact_response_body_from_create_response(body),
            },
            OpenAiCreateResponseResponse::Error {
                stats_code,
                headers,
                body,
            } => OpenAiCompactResponse::Error {
                stats_code,
                headers,
                body,
            },
        })
    }
}
