use std::collections::HashSet;

use crate::gemini::stream_generate_content::response::GeminiStreamGenerateContentResponse;
use crate::gemini::stream_generate_content::stream::GeminiSseEventData;
use crate::openai::create_image::stream::{
    ImageGenerationStreamEvent, OpenAiCreateImageSseData, OpenAiCreateImageSseEvent,
    OpenAiCreateImageSseStreamBody,
};
use crate::transform::openai::create_image::gemini::utils::{
    best_effort_openai_image_usage_from_gemini, gemini_inline_image_outputs_from_response,
};
use crate::transform::openai::create_image::utils::stream_error_from_response_error;
use crate::transform::utils::TransformError;

fn completed_image_event(
    b64_json: String,
    output_format: crate::openai::create_image::types::OpenAiImageOutputFormat,
    usage: crate::openai::create_image::types::OpenAiImageUsage,
) -> OpenAiCreateImageSseEvent {
    OpenAiCreateImageSseEvent {
        event: None,
        data: OpenAiCreateImageSseData::Event(ImageGenerationStreamEvent::Completed {
            b64_json,
            background: crate::openai::create_image::types::OpenAiImageBackground::Auto,
            created_at: 0,
            output_format,
            quality: crate::openai::create_image::types::OpenAiImageQuality::Auto,
            size: crate::openai::create_image::types::OpenAiImageSize::Auto,
            usage,
        }),
    }
}

fn error_event(code: String, message: String) -> OpenAiCreateImageSseEvent {
    OpenAiCreateImageSseEvent {
        event: None,
        data: OpenAiCreateImageSseData::Event(ImageGenerationStreamEvent::Error {
            error: stream_error_from_response_error(Some(code), message, None),
        }),
    }
}

fn done_event() -> OpenAiCreateImageSseEvent {
    OpenAiCreateImageSseEvent {
        event: None,
        data: OpenAiCreateImageSseData::Done("[DONE]".to_string()),
    }
}

impl TryFrom<GeminiStreamGenerateContentResponse> for OpenAiCreateImageSseStreamBody {
    type Error = TransformError;

    fn try_from(value: GeminiStreamGenerateContentResponse) -> Result<Self, TransformError> {
        let mut outputs = Vec::new();
        let mut seen = HashSet::new();
        let mut final_usage = None;
        let mut events = Vec::new();

        match value {
            GeminiStreamGenerateContentResponse::NdjsonSuccess { body, .. } => {
                for chunk in body.chunks {
                    if chunk.usage_metadata.is_some() {
                        final_usage = chunk.usage_metadata.clone();
                    }
                    for output in gemini_inline_image_outputs_from_response(&chunk) {
                        let key = (
                            output.candidate_index,
                            output.part_index,
                            output.b64_json.clone(),
                        );
                        if seen.insert(key) {
                            outputs.push(output);
                        }
                    }
                }
            }
            GeminiStreamGenerateContentResponse::SseSuccess { body, .. } => {
                for event in body.events {
                    match event.data {
                        GeminiSseEventData::Chunk(chunk) => {
                            if chunk.usage_metadata.is_some() {
                                final_usage = chunk.usage_metadata.clone();
                            }
                            for output in gemini_inline_image_outputs_from_response(&chunk) {
                                let key = (
                                    output.candidate_index,
                                    output.part_index,
                                    output.b64_json.clone(),
                                );
                                if seen.insert(key) {
                                    outputs.push(output);
                                }
                            }
                        }
                        GeminiSseEventData::Done(_) => {}
                    }
                }
            }
            GeminiStreamGenerateContentResponse::Error { body, .. } => {
                let code = body
                    .error
                    .status
                    .unwrap_or_else(|| body.error.code.to_string())
                    .to_lowercase();
                events.push(error_event(code, body.error.message));
                events.push(done_event());
                return Ok(Self { events });
            }
        }

        if outputs.is_empty() {
            return Err(TransformError::not_implemented(
                "cannot convert Gemini image stream without inline image output to OpenAI image stream",
            ));
        }

        let usage = best_effort_openai_image_usage_from_gemini(final_usage.as_ref());
        for output in outputs {
            events.push(completed_image_event(
                output.b64_json,
                output.output_format,
                usage.clone(),
            ));
        }
        events.push(done_event());

        Ok(Self { events })
    }
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use super::*;
    use crate::gemini::count_tokens::types as gt;
    use crate::gemini::generate_content::response::ResponseBody as GeminiGenerateContentResponseBody;
    use crate::gemini::generate_content::types as gct;
    use crate::gemini::stream_generate_content::stream::GeminiNdjsonStreamBody;
    use crate::gemini::types::{GeminiApiError, GeminiApiErrorResponse, GeminiResponseHeaders};

    #[test]
    fn converts_gemini_stream_to_openai_create_image_stream() {
        let stream = GeminiStreamGenerateContentResponse::NdjsonSuccess {
            stats_code: StatusCode::OK,
            headers: GeminiResponseHeaders::default(),
            body: GeminiNdjsonStreamBody {
                chunks: vec![
                    GeminiGenerateContentResponseBody {
                        candidates: Some(vec![gct::GeminiCandidate {
                            content: Some(gt::GeminiContent {
                                parts: vec![gt::GeminiPart {
                                    inline_data: Some(gt::GeminiBlob {
                                        mime_type: "image/png".to_string(),
                                        data: "chunk-image".to_string(),
                                    }),
                                    ..gt::GeminiPart::default()
                                }],
                                role: Some(gt::GeminiContentRole::Model),
                            }),
                            index: Some(0),
                            ..gct::GeminiCandidate::default()
                        }]),
                        ..GeminiGenerateContentResponseBody::default()
                    },
                    GeminiGenerateContentResponseBody {
                        usage_metadata: Some(gct::GeminiUsageMetadata {
                            prompt_token_count: Some(10),
                            candidates_token_count: Some(20),
                            total_token_count: Some(30),
                            ..gct::GeminiUsageMetadata::default()
                        }),
                        ..GeminiGenerateContentResponseBody::default()
                    },
                ],
            },
        };

        let converted = OpenAiCreateImageSseStreamBody::try_from(stream).unwrap();
        assert_eq!(converted.events.len(), 2);
        match &converted.events[0].data {
            OpenAiCreateImageSseData::Event(ImageGenerationStreamEvent::Completed {
                b64_json,
                output_format,
                usage,
                ..
            }) => {
                assert_eq!(b64_json, "chunk-image");
                assert_eq!(
                    output_format,
                    &crate::openai::create_image::types::OpenAiImageOutputFormat::Png
                );
                assert_eq!(usage.total_tokens, 30);
            }
            _ => panic!("expected completed image event"),
        }
        assert!(matches!(
            converted.events[1].data,
            OpenAiCreateImageSseData::Done(ref marker) if marker == "[DONE]"
        ));
    }

    #[test]
    fn converts_gemini_stream_error_to_openai_create_image_error_event() {
        let stream = GeminiStreamGenerateContentResponse::Error {
            stats_code: StatusCode::BAD_REQUEST,
            headers: GeminiResponseHeaders::default(),
            body: GeminiApiErrorResponse {
                error: GeminiApiError {
                    code: 400,
                    message: "bad stream request".to_string(),
                    status: Some("INVALID_ARGUMENT".to_string()),
                    details: None,
                },
            },
        };

        let converted = OpenAiCreateImageSseStreamBody::try_from(stream).unwrap();
        assert_eq!(converted.events.len(), 2);
        match &converted.events[0].data {
            OpenAiCreateImageSseData::Event(ImageGenerationStreamEvent::Error { error }) => {
                assert_eq!(error.message, "bad stream request");
                assert_eq!(error.code.as_deref(), Some("invalid_argument"));
            }
            _ => panic!("expected error event"),
        }
        assert!(matches!(
            converted.events[1].data,
            OpenAiCreateImageSseData::Done(ref marker) if marker == "[DONE]"
        ));
    }
}
