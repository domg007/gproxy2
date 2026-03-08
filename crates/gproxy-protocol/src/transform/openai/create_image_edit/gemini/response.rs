use crate::gemini::generate_content::response::GeminiGenerateContentResponse;
use crate::openai::create_image_edit::response::OpenAiCreateImageEditResponse;
use crate::transform::openai::create_image::gemini::utils::{
    create_image_response_body_from_gemini_response, openai_response_headers_from_gemini,
};
use crate::transform::openai::model_list::gemini::utils::openai_error_response_from_gemini;
use crate::transform::utils::TransformError;

impl TryFrom<GeminiGenerateContentResponse> for OpenAiCreateImageEditResponse {
    type Error = TransformError;

    fn try_from(value: GeminiGenerateContentResponse) -> Result<Self, TransformError> {
        Ok(match value {
            GeminiGenerateContentResponse::Success {
                stats_code,
                headers,
                body,
            } => OpenAiCreateImageEditResponse::Success {
                stats_code,
                headers: openai_response_headers_from_gemini(headers),
                body: create_image_response_body_from_gemini_response(body)?,
            },
            GeminiGenerateContentResponse::Error {
                stats_code,
                headers,
                body,
            } => OpenAiCreateImageEditResponse::Error {
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
    use crate::gemini::generate_content::response::ResponseBody as GeminiGenerateContentResponseBody;
    use crate::gemini::generate_content::types as gct;
    use crate::gemini::types::GeminiResponseHeaders;

    #[test]
    fn converts_gemini_response_to_openai_create_image_edit_response() {
        let response = GeminiGenerateContentResponse::Success {
            stats_code: StatusCode::OK,
            headers: GeminiResponseHeaders::default(),
            body: GeminiGenerateContentResponseBody {
                candidates: Some(vec![gct::GeminiCandidate {
                    content: Some(gt::GeminiContent {
                        parts: vec![gt::GeminiPart {
                            inline_data: Some(gt::GeminiBlob {
                                mime_type: "image/webp".to_string(),
                                data: "edit-image".to_string(),
                            }),
                            ..gt::GeminiPart::default()
                        }],
                        role: Some(gt::GeminiContentRole::Model),
                    }),
                    ..gct::GeminiCandidate::default()
                }]),
                ..GeminiGenerateContentResponseBody::default()
            },
        };

        let converted = OpenAiCreateImageEditResponse::try_from(response).unwrap();
        let OpenAiCreateImageEditResponse::Success { body, .. } = converted else {
            panic!("expected success response")
        };

        assert_eq!(
            body.output_format,
            Some(crate::openai::create_image::types::OpenAiImageOutputFormat::Webp)
        );
        assert_eq!(body.data.as_ref().map(Vec::len), Some(1));
        assert_eq!(
            body.data.as_ref().unwrap()[0].b64_json.as_deref(),
            Some("edit-image")
        );
    }
}
