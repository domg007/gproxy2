use crate::openai::count_tokens::types as ot;
use crate::openai::create_image_edit::request::{
    OpenAiCreateImageEditRequest, RequestBody as CreateImageEditRequestBody,
};
use crate::openai::create_image_edit::types as iet;
use crate::openai::create_response::request::{
    OpenAiCreateResponseRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::transform::openai::create_image::utils::{
    create_image_edit_model_to_string, image_tool_choice, response_image_background_from_request,
    response_image_model_from_create_image_edit_model, response_image_moderation_from_request,
    response_image_output_format_from_request,
    response_image_quality_from_create_image_edit_request,
    response_image_size_from_create_image_edit_request, user_message_from_parts,
};
use crate::transform::utils::TransformError;

fn validate_create_image_edit_request(
    body: &CreateImageEditRequestBody,
) -> Result<(), TransformError> {
    if body.n.is_some_and(|count| count > 1) {
        return Err(TransformError::not_implemented(
            "cannot convert OpenAI image edit request with n > 1 to OpenAI responses.create request",
        ));
    }

    Ok(())
}

impl TryFrom<OpenAiCreateImageEditRequest> for OpenAiCreateResponseRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateImageEditRequest) -> Result<Self, TransformError> {
        let headers = RequestHeaders {
            extra: value.headers.extra,
        };
        let body = value.body;
        validate_create_image_edit_request(&body)?;

        let top_level_model = body.model.as_ref().map(create_image_edit_model_to_string);
        let mut parts = vec![ot::ResponseInputContent::Text(ot::ResponseInputText {
            text: body.prompt,
            type_: ot::ResponseInputTextType::InputText,
        })];
        parts.extend(body.images.into_iter().map(|image| {
            ot::ResponseInputContent::Image(ot::ResponseInputImage {
                detail: None,
                type_: ot::ResponseInputImageType::InputImage,
                file_id: image.file_id,
                image_url: image.image_url,
            })
        }));

        let tool = ot::ResponseTool::ImageGeneration(ot::ResponseImageGenerationTool {
            type_: ot::ResponseImageGenerationToolType::ImageGeneration,
            action: Some(ot::ResponseImageGenerationAction::Edit),
            background: response_image_background_from_request(body.background),
            input_fidelity: match body.input_fidelity {
                Some(iet::OpenAiImageEditInputFidelity::High) => {
                    Some(ot::ResponseImageGenerationInputFidelity::High)
                }
                Some(iet::OpenAiImageEditInputFidelity::Low) => {
                    Some(ot::ResponseImageGenerationInputFidelity::Low)
                }
                None => None,
            },
            input_image_mask: body
                .mask
                .map(|mask| ot::ResponseImageGenerationInputImageMask {
                    file_id: mask.file_id,
                    image_url: mask.image_url,
                }),
            model: body
                .model
                .map(response_image_model_from_create_image_edit_model),
            moderation: response_image_moderation_from_request(body.moderation),
            output_compression: body.output_compression.map(u64::from),
            output_format: response_image_output_format_from_request(body.output_format),
            partial_images: body.partial_images,
            quality: response_image_quality_from_create_image_edit_request(body.quality),
            size: response_image_size_from_create_image_edit_request(body.size),
        });

        Ok(OpenAiCreateResponseRequest {
            method: value.method,
            path: PathParameters::default(),
            query: QueryParameters::default(),
            headers,
            body: RequestBody {
                input: Some(user_message_from_parts(parts)),
                max_tool_calls: Some(1),
                model: top_level_model,
                parallel_tool_calls: Some(false),
                stream: body.stream,
                tool_choice: Some(image_tool_choice()),
                tools: Some(vec![tool]),
                user: body.user,
                ..RequestBody::default()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::create_image::types as it;
    use crate::openai::create_image_edit::request as ciereq;

    #[test]
    fn converts_create_image_edit_request_to_openai_response_request() {
        let request = OpenAiCreateImageEditRequest {
            body: ciereq::RequestBody {
                images: vec![iet::OpenAiImageEditInputImage {
                    file_id: Some("file_123".to_string()),
                    image_url: None,
                }],
                prompt: "Turn this into a watercolor poster".to_string(),
                background: Some(it::OpenAiImageBackground::Opaque),
                input_fidelity: Some(iet::OpenAiImageEditInputFidelity::High),
                mask: Some(iet::OpenAiImageEditInputImage {
                    file_id: None,
                    image_url: Some("data:image/png;base64,mask".to_string()),
                }),
                model: Some(iet::OpenAiImageEditModel::Known(
                    iet::OpenAiImageEditModelKnown::ChatgptImageLatest,
                )),
                moderation: Some(it::OpenAiImageModeration::Auto),
                n: Some(1),
                output_compression: Some(100),
                output_format: Some(it::OpenAiImageOutputFormat::Webp),
                partial_images: Some(1),
                quality: Some(iet::OpenAiImageEditQuality::Medium),
                size: Some(iet::OpenAiImageEditSize::S1024x1536),
                stream: Some(false),
                user: Some("user-456".to_string()),
            },
            ..OpenAiCreateImageEditRequest::default()
        };

        let converted = OpenAiCreateResponseRequest::try_from(request).unwrap();
        assert_eq!(
            converted.body.model.as_deref(),
            Some("chatgpt-image-latest")
        );
        assert_eq!(converted.body.stream, Some(false));

        let Some(ot::ResponseInput::Items(items)) = converted.body.input else {
            panic!("expected input items")
        };
        let ot::ResponseInputItem::Message(message) = &items[0] else {
            panic!("expected user message")
        };
        let ot::ResponseInputMessageContent::List(parts) = &message.content else {
            panic!("expected input parts")
        };
        assert_eq!(parts.len(), 2);
        let ot::ResponseInputContent::Text(text) = &parts[0] else {
            panic!("expected prompt text first")
        };
        assert_eq!(text.text, "Turn this into a watercolor poster");
        let ot::ResponseInputContent::Image(image) = &parts[1] else {
            panic!("expected input image second")
        };
        assert_eq!(image.file_id.as_deref(), Some("file_123"));

        let Some(tools) = converted.body.tools else {
            panic!("expected tools")
        };
        let ot::ResponseTool::ImageGeneration(tool) = &tools[0] else {
            panic!("expected image_generation tool")
        };
        assert_eq!(tool.action, Some(ot::ResponseImageGenerationAction::Edit));
        assert_eq!(
            tool.input_fidelity,
            Some(ot::ResponseImageGenerationInputFidelity::High)
        );
        assert_eq!(
            tool.quality,
            Some(ot::ResponseImageGenerationQuality::Medium)
        );
        assert_eq!(tool.size, Some(ot::ResponseImageGenerationSize::S1024x1536));
        let mask = tool.input_image_mask.as_ref().expect("mask");
        assert_eq!(
            mask.image_url.as_deref(),
            Some("data:image/png;base64,mask")
        );
    }

    #[test]
    fn rejects_multiple_edit_outputs() {
        let request = OpenAiCreateImageEditRequest {
            body: ciereq::RequestBody {
                n: Some(2),
                ..ciereq::RequestBody::default()
            },
            ..OpenAiCreateImageEditRequest::default()
        };

        let error = OpenAiCreateResponseRequest::try_from(request).expect_err("expected failure");
        assert_eq!(
            error,
            TransformError::not_implemented(
                "cannot convert OpenAI image edit request with n > 1 to OpenAI responses.create request",
            )
        );
    }
}
