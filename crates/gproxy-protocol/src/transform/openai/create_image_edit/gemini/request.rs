use crate::gemini::count_tokens::types as gt;
use crate::gemini::generate_content::request::{
    GeminiGenerateContentRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::gemini::stream_generate_content::request::{
    GeminiStreamGenerateContentRequest,
    PathParameters as GeminiStreamGenerateContentPathParameters,
    QueryParameters as GeminiStreamGenerateContentQueryParameters,
    RequestHeaders as GeminiStreamGenerateContentRequestHeaders,
};
use crate::gemini::types::HttpMethod as GeminiHttpMethod;
use crate::openai::create_image_edit::request::{
    OpenAiCreateImageEditRequest, RequestBody as CreateImageEditRequestBody,
};
use crate::transform::gemini::model_get::utils::ensure_models_prefix;
use crate::transform::openai::create_image::gemini::utils::{
    gemini_image_config_from_create_image_edit_size, gemini_part_from_openai_edit_input_image,
};
use crate::transform::openai::create_image::utils::create_image_edit_model_to_string;
use crate::transform::utils::TransformError;

fn validate_create_image_edit_request(
    body: &CreateImageEditRequestBody,
) -> Result<(), TransformError> {
    if body.images.is_empty() {
        return Err(TransformError::not_implemented(
            "cannot convert OpenAI image edit request without input images to Gemini generateContent request",
        ));
    }

    if body.mask.is_some() {
        return Err(TransformError::not_implemented(
            "cannot convert OpenAI image edit request with mask to Gemini generateContent request",
        ));
    }

    Ok(())
}

impl TryFrom<OpenAiCreateImageEditRequest> for GeminiGenerateContentRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateImageEditRequest) -> Result<Self, TransformError> {
        let headers = RequestHeaders {
            extra: value.headers.extra,
        };
        let body = value.body;
        validate_create_image_edit_request(&body)?;

        let mut parts = Vec::with_capacity(body.images.len() + 1);
        for image in body.images {
            parts.push(gemini_part_from_openai_edit_input_image(image)?);
        }
        parts.push(gt::GeminiPart {
            text: Some(body.prompt),
            ..gt::GeminiPart::default()
        });

        let model = ensure_models_prefix(
            &body
                .model
                .as_ref()
                .map(create_image_edit_model_to_string)
                .unwrap_or_default(),
        );

        Ok(GeminiGenerateContentRequest {
            method: GeminiHttpMethod::Post,
            path: PathParameters { model },
            query: QueryParameters::default(),
            headers,
            body: RequestBody {
                contents: vec![gt::GeminiContent {
                    parts,
                    role: Some(gt::GeminiContentRole::User),
                }],
                tools: None,
                tool_config: None,
                safety_settings: None,
                system_instruction: None,
                generation_config: Some(gt::GeminiGenerationConfig {
                    response_modalities: Some(vec![gt::GeminiModality::Image]),
                    candidate_count: body.n,
                    image_config: gemini_image_config_from_create_image_edit_size(body.size),
                    ..gt::GeminiGenerationConfig::default()
                }),
                cached_content: None,
            },
        })
    }
}

impl TryFrom<&OpenAiCreateImageEditRequest> for GeminiStreamGenerateContentRequest {
    type Error = TransformError;

    fn try_from(value: &OpenAiCreateImageEditRequest) -> Result<Self, TransformError> {
        let output = GeminiGenerateContentRequest::try_from(value.clone())?;

        Ok(Self {
            method: GeminiHttpMethod::Post,
            path: GeminiStreamGenerateContentPathParameters {
                model: output.path.model,
            },
            query: GeminiStreamGenerateContentQueryParameters::default(),
            headers: GeminiStreamGenerateContentRequestHeaders {
                extra: output.headers.extra,
            },
            body: output.body,
        })
    }
}

impl TryFrom<OpenAiCreateImageEditRequest> for GeminiStreamGenerateContentRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateImageEditRequest) -> Result<Self, TransformError> {
        GeminiStreamGenerateContentRequest::try_from(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::create_image_edit::request as ciereq;
    use crate::openai::create_image_edit::types as iet;

    #[test]
    fn converts_create_image_edit_request_to_gemini_generate_content_request() {
        let request = OpenAiCreateImageEditRequest {
            body: ciereq::RequestBody {
                images: vec![iet::OpenAiImageEditInputImage {
                    file_id: None,
                    image_url: Some("data:image/png;base64,abc123".to_string()),
                }],
                prompt: "Turn this into stained glass".to_string(),
                model: Some(iet::OpenAiImageEditModel::Custom(
                    "gemini-2.5-flash-image".to_string(),
                )),
                n: Some(2),
                size: Some(iet::OpenAiImageEditSize::S1024x1536),
                ..ciereq::RequestBody::default()
            },
            ..OpenAiCreateImageEditRequest::default()
        };

        let converted = GeminiGenerateContentRequest::try_from(request).unwrap();
        assert_eq!(converted.path.model, "models/gemini-2.5-flash-image");
        assert_eq!(converted.body.contents.len(), 1);
        assert_eq!(converted.body.contents[0].parts.len(), 2);
        assert_eq!(
            converted.body.contents[0].parts[0]
                .inline_data
                .as_ref()
                .map(|value| value.mime_type.as_str()),
            Some("image/png")
        );
        assert_eq!(
            converted.body.contents[0].parts[1].text.as_deref(),
            Some("Turn this into stained glass")
        );

        let config = converted.body.generation_config.expect("generation config");
        assert_eq!(config.candidate_count, Some(2));
        assert_eq!(
            config.response_modalities,
            Some(vec![gt::GeminiModality::Image])
        );
        let image_config = config.image_config.expect("image config");
        assert_eq!(image_config.aspect_ratio.as_deref(), Some("2:3"));
    }

    #[test]
    fn converts_create_image_edit_request_to_gemini_stream_generate_content_request() {
        let request = OpenAiCreateImageEditRequest {
            body: ciereq::RequestBody {
                images: vec![iet::OpenAiImageEditInputImage {
                    file_id: None,
                    image_url: Some(
                        "https://generativelanguage.googleapis.com/v1beta/files/123".to_string(),
                    ),
                }],
                prompt: "Make it cinematic".to_string(),
                model: Some(iet::OpenAiImageEditModel::Custom(
                    "gemini-2.5-flash-image".to_string(),
                )),
                stream: Some(true),
                ..ciereq::RequestBody::default()
            },
            ..OpenAiCreateImageEditRequest::default()
        };

        let converted = GeminiStreamGenerateContentRequest::try_from(&request).unwrap();
        assert_eq!(converted.path.model, "models/gemini-2.5-flash-image");
        assert_eq!(converted.body.contents[0].parts.len(), 2);
        assert_eq!(
            converted.body.contents[0].parts[0]
                .file_data
                .as_ref()
                .map(|value| value.file_uri.as_str()),
            Some("https://generativelanguage.googleapis.com/v1beta/files/123")
        );
    }

    #[test]
    fn rejects_masked_edit_requests() {
        let request = OpenAiCreateImageEditRequest {
            body: ciereq::RequestBody {
                images: vec![iet::OpenAiImageEditInputImage {
                    file_id: None,
                    image_url: Some("data:image/png;base64,abc123".to_string()),
                }],
                mask: Some(iet::OpenAiImageEditInputImage {
                    file_id: None,
                    image_url: Some("data:image/png;base64,mask456".to_string()),
                }),
                ..ciereq::RequestBody::default()
            },
            ..OpenAiCreateImageEditRequest::default()
        };

        let error = GeminiGenerateContentRequest::try_from(request).expect_err("expected failure");
        assert_eq!(
            error,
            TransformError::not_implemented(
                "cannot convert OpenAI image edit request with mask to Gemini generateContent request",
            )
        );
    }
}
