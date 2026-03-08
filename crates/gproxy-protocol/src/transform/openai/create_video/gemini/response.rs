use crate::gemini::generate_videos::response::GeminiGenerateVideosResponse;
use crate::openai::create_video::response::OpenAiCreateVideoResponse;
use crate::transform::openai::create_image::gemini::utils::openai_response_headers_from_gemini;
use crate::transform::openai::create_video::utils::openai_video_from_gemini_operation;
use crate::transform::openai::model_list::gemini::utils::openai_error_response_from_gemini;

impl TryFrom<GeminiGenerateVideosResponse> for OpenAiCreateVideoResponse {
    type Error = crate::transform::utils::TransformError;

    fn try_from(
        value: GeminiGenerateVideosResponse,
    ) -> Result<Self, crate::transform::utils::TransformError> {
        Ok(match value {
            GeminiGenerateVideosResponse::Success {
                stats_code,
                headers,
                body,
            } => OpenAiCreateVideoResponse::Success {
                stats_code,
                headers: openai_response_headers_from_gemini(headers),
                body: openai_video_from_gemini_operation(*body),
            },
            GeminiGenerateVideosResponse::Error {
                stats_code,
                headers,
                body,
            } => OpenAiCreateVideoResponse::Error {
                stats_code,
                headers: openai_response_headers_from_gemini(headers),
                body: openai_error_response_from_gemini(stats_code, body),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use super::*;
    use crate::gemini::generate_videos::response::ResponseBody as GeminiResponseBody;
    use crate::gemini::generate_videos::types::{
        GeminiGenerateVideoResponse, GeminiGenerateVideosOperationResult,
        GeminiGeneratedVideoSample, GeminiResponseHeaders, GeminiVideoFile,
        GeminiVideoOperationMetadata,
    };

    #[test]
    fn converts_gemini_generate_videos_response_to_openai_video_response() {
        let response = GeminiGenerateVideosResponse::Success {
            stats_code: StatusCode::OK,
            headers: GeminiResponseHeaders::default(),
            body: Box::new(GeminiResponseBody {
                name: "operations/abc123".to_string(),
                metadata: Some(GeminiVideoOperationMetadata {
                    create_time: Some("2026-03-08T12:00:00Z".to_string()),
                    end_time: Some("2026-03-08T12:00:08Z".to_string()),
                    model: Some("models/veo-3.1-generate-preview".to_string()),
                    prompt: Some("A tiny submarine made of glass".to_string()),
                    aspect_ratio: Some("16:9".to_string()),
                    duration_seconds: Some("8".to_string()),
                    progress_percent: Some(100.0),
                    ..GeminiVideoOperationMetadata::default()
                }),
                done: Some(true),
                response: Some(GeminiGenerateVideosOperationResult {
                    generate_video_response: Some(GeminiGenerateVideoResponse {
                        generated_samples: Some(vec![GeminiGeneratedVideoSample {
                            video: Some(GeminiVideoFile {
                                uri: Some("https://example.com/video.mp4".to_string()),
                                mime_type: Some("video/mp4".to_string()),
                            }),
                        }]),
                        ..GeminiGenerateVideoResponse::default()
                    }),
                    ..GeminiGenerateVideosOperationResult::default()
                }),
                error: None,
            }),
        };

        let converted = OpenAiCreateVideoResponse::try_from(response).unwrap();
        let OpenAiCreateVideoResponse::Success { body, .. } = converted else {
            panic!("expected success response");
        };
        assert_eq!(body.prompt, "A tiny submarine made of glass");
        assert_eq!(body.seconds, "8");
        assert_eq!(body.progress, 100.0);
        assert_eq!(
            body.status,
            crate::openai::create_video::types::OpenAiVideoStatus::Completed
        );
    }
}
