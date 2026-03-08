use crate::gemini::generate_content::response::{
    GeminiGenerateContentResponse, ResponseBody as GeminiGenerateContentResponseBody,
};
use crate::openai::create_image::response::OpenAiCreateImageResponse;
use crate::openai::create_image::types as it;
use crate::transform::openai::create_image::gemini::utils::{
    create_image_response_body_from_gemini_response, openai_response_headers_from_gemini,
};
use crate::transform::openai::model_list::gemini::utils::openai_error_response_from_gemini;
use crate::transform::utils::TransformError;

impl TryFrom<GeminiGenerateContentResponseBody> for it::OpenAiCreateImageResponseBody {
    type Error = TransformError;

    fn try_from(value: GeminiGenerateContentResponseBody) -> Result<Self, TransformError> {
        create_image_response_body_from_gemini_response(value)
    }
}

impl TryFrom<GeminiGenerateContentResponse> for OpenAiCreateImageResponse {
    type Error = TransformError;

    fn try_from(value: GeminiGenerateContentResponse) -> Result<Self, TransformError> {
        Ok(match value {
            GeminiGenerateContentResponse::Success {
                stats_code,
                headers,
                body,
            } => OpenAiCreateImageResponse::Success {
                stats_code,
                headers: openai_response_headers_from_gemini(headers),
                body: it::OpenAiCreateImageResponseBody::try_from(body)?,
            },
            GeminiGenerateContentResponse::Error {
                stats_code,
                headers,
                body,
            } => OpenAiCreateImageResponse::Error {
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
    use crate::gemini::count_tokens::types as gt;
    use crate::gemini::generate_content::types as gct;
    use crate::gemini::types::{GeminiApiError, GeminiApiErrorResponse, GeminiResponseHeaders};

    #[test]
    fn converts_gemini_response_to_openai_create_image_response() {
        let response = GeminiGenerateContentResponse::Success {
            stats_code: StatusCode::OK,
            headers: GeminiResponseHeaders::default(),
            body: GeminiGenerateContentResponseBody {
                candidates: Some(vec![gct::GeminiCandidate {
                    content: Some(gt::GeminiContent {
                        parts: vec![gt::GeminiPart {
                            inline_data: Some(gt::GeminiBlob {
                                mime_type: "image/png".to_string(),
                                data: "abc123".to_string(),
                            }),
                            ..gt::GeminiPart::default()
                        }],
                        role: Some(gt::GeminiContentRole::Model),
                    }),
                    index: Some(0),
                    ..gct::GeminiCandidate::default()
                }]),
                usage_metadata: Some(gct::GeminiUsageMetadata {
                    prompt_token_count: Some(12),
                    candidates_token_count: Some(34),
                    total_token_count: Some(46),
                    prompt_tokens_details: Some(vec![
                        gt::GeminiModalityTokenCount {
                            modality: gt::GeminiModality::Image,
                            token_count: 3,
                        },
                        gt::GeminiModalityTokenCount {
                            modality: gt::GeminiModality::Text,
                            token_count: 9,
                        },
                    ]),
                    candidates_tokens_details: Some(vec![
                        gt::GeminiModalityTokenCount {
                            modality: gt::GeminiModality::Image,
                            token_count: 30,
                        },
                        gt::GeminiModalityTokenCount {
                            modality: gt::GeminiModality::Text,
                            token_count: 4,
                        },
                    ]),
                    ..gct::GeminiUsageMetadata::default()
                }),
                ..GeminiGenerateContentResponseBody::default()
            },
        };

        let converted = OpenAiCreateImageResponse::try_from(response).unwrap();
        let OpenAiCreateImageResponse::Success { body, .. } = converted else {
            panic!("expected success response")
        };

        assert_eq!(body.created, 0);
        assert_eq!(body.output_format, Some(it::OpenAiImageOutputFormat::Png));
        assert_eq!(body.data.as_ref().map(Vec::len), Some(1));
        assert_eq!(
            body.data.as_ref().unwrap()[0].b64_json.as_deref(),
            Some("abc123")
        );

        let usage = body.usage.expect("usage");
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 34);
        assert_eq!(usage.total_tokens, 46);
        assert_eq!(usage.input_tokens_details.image_tokens, 3);
        assert_eq!(usage.input_tokens_details.text_tokens, 9);
        assert_eq!(
            usage.output_tokens_details.as_ref().unwrap().image_tokens,
            30
        );
    }

    #[test]
    fn converts_gemini_error_to_openai_error_response() {
        let response = GeminiGenerateContentResponse::Error {
            stats_code: StatusCode::BAD_REQUEST,
            headers: GeminiResponseHeaders::default(),
            body: GeminiApiErrorResponse {
                error: GeminiApiError {
                    code: 400,
                    message: "bad image request".to_string(),
                    status: Some("INVALID_ARGUMENT".to_string()),
                    details: None,
                },
            },
        };

        let converted = OpenAiCreateImageResponse::try_from(response).unwrap();
        let OpenAiCreateImageResponse::Error {
            stats_code, body, ..
        } = converted
        else {
            panic!("expected error response")
        };

        assert_eq!(stats_code, StatusCode::BAD_REQUEST);
        assert_eq!(body.error.type_, "invalid_request_error");
        assert_eq!(body.error.message, "bad image request");
        assert_eq!(body.error.code.as_deref(), Some("INVALID_ARGUMENT"));
    }
}
