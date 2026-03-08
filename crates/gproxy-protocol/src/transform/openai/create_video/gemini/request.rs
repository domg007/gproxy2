use crate::gemini::generate_videos::request::{
    GeminiGenerateVideosRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::gemini::generate_videos::types::{
    GeminiGenerateVideosInstance, GeminiGenerateVideosParameters,
};
use crate::gemini::types::HttpMethod as GeminiHttpMethod;
use crate::openai::create_video::request::{
    OpenAiCreateVideoRequest, RequestBody as OpenAiCreateVideoRequestBody,
};
use crate::openai::create_video::types::{OpenAiVideoSeconds, OpenAiVideoSize};
use crate::transform::gemini::model_get::utils::ensure_models_prefix;
use crate::transform::openai::create_video::utils::{
    gemini_aspect_ratio_from_openai_size, gemini_video_asset_from_image_url,
    openai_video_model_to_string,
};
use crate::transform::utils::TransformError;

fn validate_request(body: &OpenAiCreateVideoRequestBody) -> Result<(), TransformError> {
    if body.input_reference.is_some() {
        return Err(TransformError::not_implemented(
            "cannot convert OpenAI video request with input_reference to Gemini Veo request",
        ));
    }

    if body
        .image_reference
        .as_ref()
        .is_some_and(|value| value.file_id.is_some())
    {
        return Err(TransformError::not_implemented(
            "cannot convert OpenAI video request with file_id image_reference to Gemini Veo request",
        ));
    }

    if matches!(body.seconds, Some(OpenAiVideoSeconds::S12)) {
        return Err(TransformError::not_implemented(
            "cannot convert OpenAI video request with seconds=12 to Gemini Veo request",
        ));
    }

    Ok(())
}

fn gemini_duration_seconds(value: Option<OpenAiVideoSeconds>) -> Option<String> {
    match value {
        Some(OpenAiVideoSeconds::S4) => Some("4".to_string()),
        Some(OpenAiVideoSeconds::S8) => Some("8".to_string()),
        Some(OpenAiVideoSeconds::S12) | None => None,
    }
}

impl TryFrom<OpenAiCreateVideoRequest> for GeminiGenerateVideosRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateVideoRequest) -> Result<Self, TransformError> {
        let headers = RequestHeaders {
            extra: value.headers.extra,
        };
        let body = value.body;
        validate_request(&body)?;

        let image = body
            .image_reference
            .and_then(|value| value.image_url)
            .map(|value| gemini_video_asset_from_image_url(&value))
            .transpose()?;

        let model = ensure_models_prefix(
            &body
                .model
                .as_ref()
                .map(openai_video_model_to_string)
                .unwrap_or_else(|| "veo-3.1-generate-preview".to_string()),
        );

        let aspect_ratio = body.size.clone().map(gemini_video_size_to_aspect_ratio);

        Ok(GeminiGenerateVideosRequest {
            method: GeminiHttpMethod::Post,
            path: PathParameters { model },
            query: QueryParameters::default(),
            headers,
            body: RequestBody {
                instances: vec![GeminiGenerateVideosInstance {
                    prompt: body.prompt,
                    image,
                    last_frame: None,
                    reference_images: None,
                    video: None,
                }],
                parameters: Some(GeminiGenerateVideosParameters {
                    aspect_ratio,
                    duration_seconds: gemini_duration_seconds(body.seconds),
                    resolution: gemini_resolution_from_openai_size(body.size),
                    ..GeminiGenerateVideosParameters::default()
                }),
            },
        })
    }
}

fn gemini_video_size_to_aspect_ratio(size: OpenAiVideoSize) -> String {
    gemini_aspect_ratio_from_openai_size(size).to_string()
}

fn gemini_resolution_from_openai_size(size: Option<OpenAiVideoSize>) -> Option<String> {
    match size {
        Some(OpenAiVideoSize::S720x1280) | Some(OpenAiVideoSize::S1280x720) => {
            Some("720p".to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::create_video::request as ovreq;
    use crate::openai::create_video::types::{OpenAiVideoImageReference, OpenAiVideoModel};

    #[test]
    fn converts_openai_video_request_to_gemini_generate_videos_request() {
        let request = OpenAiCreateVideoRequest {
            body: ovreq::RequestBody {
                prompt: "A paper airplane gliding above neon rivers".to_string(),
                image_reference: Some(OpenAiVideoImageReference {
                    file_id: None,
                    image_url: Some("data:image/png;base64,abc123".to_string()),
                }),
                model: Some(OpenAiVideoModel::Custom(
                    "veo-3.1-generate-preview".to_string(),
                )),
                seconds: Some(OpenAiVideoSeconds::S8),
                size: Some(OpenAiVideoSize::S1280x720),
                ..ovreq::RequestBody::default()
            },
            ..OpenAiCreateVideoRequest::default()
        };

        let converted = GeminiGenerateVideosRequest::try_from(request).unwrap();
        assert_eq!(converted.path.model, "models/veo-3.1-generate-preview");
        assert_eq!(converted.body.instances.len(), 1);
        assert_eq!(
            converted.body.instances[0].prompt,
            "A paper airplane gliding above neon rivers"
        );
        assert_eq!(
            converted.body.instances[0]
                .image
                .as_ref()
                .and_then(|value| value.inline_data.as_ref())
                .map(|value| value.mime_type.as_str()),
            Some("image/png")
        );
        let parameters = converted.body.parameters.expect("parameters");
        assert_eq!(parameters.aspect_ratio.as_deref(), Some("16:9"));
        assert_eq!(parameters.duration_seconds.as_deref(), Some("8"));
        assert_eq!(parameters.resolution.as_deref(), Some("720p"));
    }

    #[test]
    fn rejects_unsupported_openai_video_request_fields() {
        let request = OpenAiCreateVideoRequest {
            body: ovreq::RequestBody {
                prompt: "demo".to_string(),
                input_reference: Some("file_ref_123".to_string()),
                ..ovreq::RequestBody::default()
            },
            ..OpenAiCreateVideoRequest::default()
        };

        let error = GeminiGenerateVideosRequest::try_from(request).expect_err("expected failure");
        assert_eq!(
            error,
            TransformError::not_implemented(
                "cannot convert OpenAI video request with input_reference to Gemini Veo request",
            )
        );
    }
}
