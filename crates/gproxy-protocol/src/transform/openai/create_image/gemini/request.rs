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
use crate::openai::create_image::request::{
    OpenAiCreateImageRequest, RequestBody as CreateImageRequestBody,
};
use crate::openai::create_image::types as it;
use crate::transform::gemini::model_get::utils::ensure_models_prefix;
use crate::transform::openai::create_image::gemini::utils::gemini_image_config_from_create_image_size;
use crate::transform::openai::create_image::utils::create_image_model_to_string;
use crate::transform::utils::TransformError;

fn validate_create_image_request(body: &CreateImageRequestBody) -> Result<(), TransformError> {
    if matches!(
        body.response_format,
        Some(it::OpenAiImageResponseFormat::Url)
    ) {
        return Err(TransformError::not_implemented(
            "cannot convert OpenAI image request with response_format=url to Gemini generateContent request",
        ));
    }

    Ok(())
}

impl TryFrom<OpenAiCreateImageRequest> for GeminiGenerateContentRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateImageRequest) -> Result<Self, TransformError> {
        let headers = RequestHeaders {
            extra: value.headers.extra,
        };
        let body = value.body;
        validate_create_image_request(&body)?;

        let image_config = gemini_image_config_from_create_image_size(body.size)?;
        let model = ensure_models_prefix(
            &body
                .model
                .as_ref()
                .map(create_image_model_to_string)
                .unwrap_or_default(),
        );

        Ok(GeminiGenerateContentRequest {
            method: GeminiHttpMethod::Post,
            path: PathParameters { model },
            query: QueryParameters::default(),
            headers,
            body: RequestBody {
                contents: vec![gt::GeminiContent {
                    parts: vec![gt::GeminiPart {
                        text: Some(body.prompt),
                        ..gt::GeminiPart::default()
                    }],
                    role: Some(gt::GeminiContentRole::User),
                }],
                tools: None,
                tool_config: None,
                safety_settings: None,
                system_instruction: None,
                generation_config: Some(gt::GeminiGenerationConfig {
                    response_modalities: Some(vec![gt::GeminiModality::Image]),
                    candidate_count: body.n,
                    image_config,
                    ..gt::GeminiGenerationConfig::default()
                }),
                cached_content: None,
            },
        })
    }
}

impl TryFrom<&OpenAiCreateImageRequest> for GeminiStreamGenerateContentRequest {
    type Error = TransformError;

    fn try_from(value: &OpenAiCreateImageRequest) -> Result<Self, TransformError> {
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

impl TryFrom<OpenAiCreateImageRequest> for GeminiStreamGenerateContentRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateImageRequest) -> Result<Self, TransformError> {
        GeminiStreamGenerateContentRequest::try_from(&value)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::openai::create_image::request as cireq;

    #[test]
    fn converts_create_image_request_to_gemini_generate_content_request() {
        let request = OpenAiCreateImageRequest {
            headers: crate::openai::create_image::request::RequestHeaders {
                extra: BTreeMap::from([("x-test".to_string(), "1".to_string())]),
            },
            body: cireq::RequestBody {
                prompt: "A glowing crystal fox".to_string(),
                model: Some(it::OpenAiImageModel::Custom(
                    "gemini-2.5-flash-image".to_string(),
                )),
                n: Some(2),
                quality: Some(it::OpenAiImageQuality::High),
                response_format: Some(it::OpenAiImageResponseFormat::B64Json),
                size: Some(it::OpenAiImageSize::S1536x1024),
                ..cireq::RequestBody::default()
            },
            ..OpenAiCreateImageRequest::default()
        };

        let converted = GeminiGenerateContentRequest::try_from(request).unwrap();
        assert_eq!(converted.path.model, "models/gemini-2.5-flash-image");
        assert_eq!(
            converted.headers.extra.get("x-test").map(String::as_str),
            Some("1")
        );
        assert_eq!(converted.body.contents.len(), 1);
        assert_eq!(
            converted.body.contents[0].parts[0].text.as_deref(),
            Some("A glowing crystal fox")
        );

        let config = converted.body.generation_config.expect("generation config");
        assert_eq!(
            config.response_modalities,
            Some(vec![gt::GeminiModality::Image])
        );
        assert_eq!(config.candidate_count, Some(2));
        let image_config = config.image_config.expect("image config");
        assert_eq!(image_config.aspect_ratio.as_deref(), Some("3:2"));
        assert_eq!(image_config.image_size.as_deref(), Some("1K"));
    }

    #[test]
    fn converts_create_image_request_to_gemini_stream_generate_content_request() {
        let request = OpenAiCreateImageRequest {
            body: cireq::RequestBody {
                prompt: "A paper lantern whale".to_string(),
                model: Some(it::OpenAiImageModel::Custom(
                    "gemini-2.5-flash-image".to_string(),
                )),
                size: Some(it::OpenAiImageSize::S1024x1024),
                stream: Some(true),
                ..cireq::RequestBody::default()
            },
            ..OpenAiCreateImageRequest::default()
        };

        let converted = GeminiStreamGenerateContentRequest::try_from(&request).unwrap();
        assert_eq!(converted.path.model, "models/gemini-2.5-flash-image");
        let config = converted.body.generation_config.expect("generation config");
        assert_eq!(
            config.response_modalities,
            Some(vec![gt::GeminiModality::Image])
        );
        assert_eq!(
            config.image_config.and_then(|value| value.aspect_ratio),
            Some("1:1".to_string())
        );
    }

    #[test]
    fn rejects_response_format_url() {
        let request = OpenAiCreateImageRequest {
            body: cireq::RequestBody {
                response_format: Some(it::OpenAiImageResponseFormat::Url),
                ..cireq::RequestBody::default()
            },
            ..OpenAiCreateImageRequest::default()
        };

        let error = GeminiGenerateContentRequest::try_from(request).expect_err("expected failure");
        assert_eq!(
            error,
            TransformError::not_implemented(
                "cannot convert OpenAI image request with response_format=url to Gemini generateContent request",
            )
        );
    }
}
