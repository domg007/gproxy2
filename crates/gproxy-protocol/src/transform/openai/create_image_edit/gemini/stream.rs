use std::collections::HashSet;

use crate::gemini::stream_generate_content::response::GeminiStreamGenerateContentResponse;
use crate::gemini::stream_generate_content::stream::GeminiSseEventData;
use crate::openai::create_image_edit::stream::{
    ImageEditStreamEvent, OpenAiCreateImageEditSseData, OpenAiCreateImageEditSseEvent,
    OpenAiCreateImageEditSseStreamBody,
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
) -> OpenAiCreateImageEditSseEvent {
    OpenAiCreateImageEditSseEvent {
        event: None,
        data: OpenAiCreateImageEditSseData::Event(ImageEditStreamEvent::Completed {
            b64_json,
            background: crate::openai::create_image::types::OpenAiImageBackground::Auto,
            created_at: 0,
            output_format,
            quality: crate::openai::create_image_edit::types::OpenAiImageEditQuality::Auto,
            size: crate::openai::create_image_edit::types::OpenAiImageEditSize::Auto,
            usage,
        }),
    }
}

fn error_event(code: String, message: String) -> OpenAiCreateImageEditSseEvent {
    OpenAiCreateImageEditSseEvent {
        event: None,
        data: OpenAiCreateImageEditSseData::Event(ImageEditStreamEvent::Error {
            error: stream_error_from_response_error(Some(code), message, None),
        }),
    }
}

fn done_event() -> OpenAiCreateImageEditSseEvent {
    OpenAiCreateImageEditSseEvent {
        event: None,
        data: OpenAiCreateImageEditSseData::Done("[DONE]".to_string()),
    }
}

impl TryFrom<GeminiStreamGenerateContentResponse> for OpenAiCreateImageEditSseStreamBody {
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
                "cannot convert Gemini image edit stream without inline image output to OpenAI image edit stream",
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
    use crate::gemini::stream_generate_content::stream::{GeminiSseEvent, GeminiSseStreamBody};
    use crate::gemini::types::GeminiResponseHeaders;

    #[test]
    fn converts_gemini_stream_to_openai_create_image_edit_stream() {
        let stream = GeminiStreamGenerateContentResponse::SseSuccess {
            stats_code: StatusCode::OK,
            headers: GeminiResponseHeaders::default(),
            body: GeminiSseStreamBody {
                events: vec![
                    GeminiSseEvent {
                        event: None,
                        data: GeminiSseEventData::Chunk(GeminiGenerateContentResponseBody {
                            candidates: Some(vec![gct::GeminiCandidate {
                                content: Some(gt::GeminiContent {
                                    parts: vec![gt::GeminiPart {
                                        inline_data: Some(gt::GeminiBlob {
                                            mime_type: "image/jpeg".to_string(),
                                            data: "edit-stream-image".to_string(),
                                        }),
                                        ..gt::GeminiPart::default()
                                    }],
                                    role: Some(gt::GeminiContentRole::Model),
                                }),
                                ..gct::GeminiCandidate::default()
                            }]),
                            usage_metadata: Some(gct::GeminiUsageMetadata {
                                prompt_token_count: Some(8),
                                candidates_token_count: Some(12),
                                total_token_count: Some(20),
                                ..gct::GeminiUsageMetadata::default()
                            }),
                            ..GeminiGenerateContentResponseBody::default()
                        }),
                    },
                    GeminiSseEvent {
                        event: None,
                        data: GeminiSseEventData::Done("[DONE]".to_string()),
                    },
                ],
            },
        };

        let converted = OpenAiCreateImageEditSseStreamBody::try_from(stream).unwrap();
        assert_eq!(converted.events.len(), 2);
        match &converted.events[0].data {
            OpenAiCreateImageEditSseData::Event(ImageEditStreamEvent::Completed {
                b64_json,
                output_format,
                usage,
                ..
            }) => {
                assert_eq!(b64_json, "edit-stream-image");
                assert_eq!(
                    output_format,
                    &crate::openai::create_image::types::OpenAiImageOutputFormat::Jpeg
                );
                assert_eq!(usage.total_tokens, 20);
            }
            _ => panic!("expected completed edit image event"),
        }
        assert!(matches!(
            converted.events[1].data,
            OpenAiCreateImageEditSseData::Done(ref marker) if marker == "[DONE]"
        ));
    }
}
