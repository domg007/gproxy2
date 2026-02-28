use crate::gemini::generate_content::response::GeminiGenerateContentResponse;
use crate::gemini::stream_generate_content::response::GeminiStreamGenerateContentResponse;
use crate::gemini::stream_generate_content::stream::GeminiSseStreamBody;
use crate::transform::gemini::stream_generate_content::utils::{chunk_event, done_event};
use crate::transform::utils::TransformError;

impl TryFrom<GeminiGenerateContentResponse> for GeminiStreamGenerateContentResponse {
    type Error = TransformError;

    fn try_from(value: GeminiGenerateContentResponse) -> Result<Self, TransformError> {
        Ok(match value {
            GeminiGenerateContentResponse::Success {
                stats_code,
                headers,
                body,
            } => GeminiStreamGenerateContentResponse::SseSuccess {
                stats_code,
                headers,
                body: GeminiSseStreamBody {
                    events: vec![chunk_event(body), done_event()],
                },
            },
            GeminiGenerateContentResponse::Error {
                stats_code,
                headers,
                body,
            } => GeminiStreamGenerateContentResponse::Error {
                stats_code,
                headers,
                body,
            },
        })
    }
}
