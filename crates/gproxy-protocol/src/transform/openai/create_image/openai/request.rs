use crate::openai::count_tokens::types as ot;
use crate::openai::create_image::request::{
    OpenAiCreateImageRequest, RequestBody as CreateImageRequestBody,
};
use crate::openai::create_image::types as it;
use crate::openai::create_response::request::{
    OpenAiCreateResponseRequest, PathParameters, QueryParameters, RequestBody, RequestHeaders,
};
use crate::transform::openai::create_image::utils::{
    create_image_model_to_string, image_tool_choice, response_image_background_from_request,
    response_image_model_from_create_image_model, response_image_moderation_from_request,
    response_image_output_format_from_request, response_image_quality_from_create_image_request,
    response_image_size_from_create_image_request, user_message_from_parts,
};
use crate::transform::utils::TransformError;

fn validate_create_image_request(body: &CreateImageRequestBody) -> Result<(), TransformError> {
    if matches!(
        body.response_format,
        Some(it::OpenAiImageResponseFormat::Url)
    ) {
        return Err(TransformError::not_implemented(
            "cannot convert OpenAI image request with response_format=url to OpenAI responses.create request",
        ));
    }

    if body.style.is_some() {
        return Err(TransformError::not_implemented(
            "cannot convert OpenAI image request with style to OpenAI responses.create request",
        ));
    }

    if body.n.is_some_and(|count| count > 1) {
        return Err(TransformError::not_implemented(
            "cannot convert OpenAI image request with n > 1 to OpenAI responses.create request",
        ));
    }

    Ok(())
}

impl TryFrom<OpenAiCreateImageRequest> for OpenAiCreateResponseRequest {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateImageRequest) -> Result<Self, TransformError> {
        let headers = RequestHeaders {
            extra: value.headers.extra,
        };
        let body = value.body;
        validate_create_image_request(&body)?;

        let top_level_model = body.model.as_ref().map(create_image_model_to_string);
        let tool = ot::ResponseTool::ImageGeneration(ot::ResponseImageGenerationTool {
            type_: ot::ResponseImageGenerationToolType::ImageGeneration,
            action: Some(ot::ResponseImageGenerationAction::Generate),
            background: response_image_background_from_request(body.background),
            input_fidelity: None,
            input_image_mask: None,
            model: body.model.map(response_image_model_from_create_image_model),
            moderation: response_image_moderation_from_request(body.moderation),
            output_compression: body.output_compression.map(u64::from),
            output_format: response_image_output_format_from_request(body.output_format),
            partial_images: body.partial_images,
            quality: response_image_quality_from_create_image_request(body.quality),
            size: response_image_size_from_create_image_request(body.size)?,
        });

        Ok(OpenAiCreateResponseRequest {
            method: value.method,
            path: PathParameters::default(),
            query: QueryParameters::default(),
            headers,
            body: RequestBody {
                input: Some(user_message_from_parts(vec![
                    ot::ResponseInputContent::Text(ot::ResponseInputText {
                        text: body.prompt,
                        type_: ot::ResponseInputTextType::InputText,
                    }),
                ])),
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
    use crate::openai::create_image::request as cireq;

    #[test]
    fn converts_create_image_request_to_openai_response_request() {
        let request = OpenAiCreateImageRequest {
            body: cireq::RequestBody {
                prompt: "A small fox made of clouds".to_string(),
                background: Some(it::OpenAiImageBackground::Transparent),
                model: Some(it::OpenAiImageModel::Known(
                    it::OpenAiImageModelKnown::GptImage1,
                )),
                moderation: Some(it::OpenAiImageModeration::Low),
                n: Some(1),
                output_compression: Some(90),
                output_format: Some(it::OpenAiImageOutputFormat::Png),
                partial_images: Some(2),
                quality: Some(it::OpenAiImageQuality::High),
                response_format: Some(it::OpenAiImageResponseFormat::B64Json),
                size: Some(it::OpenAiImageSize::S1024x1024),
                stream: Some(true),
                style: None,
                user: Some("user-123".to_string()),
            },
            ..OpenAiCreateImageRequest::default()
        };

        let converted = OpenAiCreateResponseRequest::try_from(request).unwrap();
        assert_eq!(converted.body.model.as_deref(), Some("gpt-image-1"));
        assert_eq!(converted.body.stream, Some(true));
        assert_eq!(converted.body.max_tool_calls, Some(1));
        assert_eq!(converted.body.parallel_tool_calls, Some(false));
        assert_eq!(converted.body.user.as_deref(), Some("user-123"));

        let Some(ot::ResponseInput::Items(items)) = converted.body.input else {
            panic!("expected input items")
        };
        assert_eq!(items.len(), 1);
        let ot::ResponseInputItem::Message(message) = &items[0] else {
            panic!("expected user message")
        };
        assert_eq!(message.role, ot::ResponseInputMessageRole::User);
        let ot::ResponseInputMessageContent::List(parts) = &message.content else {
            panic!("expected input parts")
        };
        assert_eq!(parts.len(), 1);
        let ot::ResponseInputContent::Text(text) = &parts[0] else {
            panic!("expected input text")
        };
        assert_eq!(text.text, "A small fox made of clouds");

        let Some(ot::ResponseToolChoice::Types(choice)) = converted.body.tool_choice else {
            panic!("expected builtin tool choice")
        };
        assert_eq!(
            choice.type_,
            ot::ResponseToolChoiceBuiltinType::ImageGeneration
        );

        let Some(tools) = converted.body.tools else {
            panic!("expected tools")
        };
        assert_eq!(tools.len(), 1);
        let ot::ResponseTool::ImageGeneration(tool) = &tools[0] else {
            panic!("expected image_generation tool")
        };
        assert_eq!(
            tool.action,
            Some(ot::ResponseImageGenerationAction::Generate)
        );
        assert_eq!(tool.output_compression, Some(90));
        assert_eq!(tool.partial_images, Some(2));
        assert_eq!(tool.quality, Some(ot::ResponseImageGenerationQuality::High));
        assert_eq!(tool.size, Some(ot::ResponseImageGenerationSize::S1024x1024));
    }

    #[test]
    fn rejects_url_response_format_for_create_image_request() {
        let request = OpenAiCreateImageRequest {
            body: cireq::RequestBody {
                prompt: "A city skyline".to_string(),
                response_format: Some(it::OpenAiImageResponseFormat::Url),
                ..cireq::RequestBody::default()
            },
            ..OpenAiCreateImageRequest::default()
        };

        let error = OpenAiCreateResponseRequest::try_from(request).expect_err("expected failure");
        assert_eq!(
            error,
            TransformError::not_implemented(
                "cannot convert OpenAI image request with response_format=url to OpenAI responses.create request",
            )
        );
    }

    #[test]
    fn rejects_unsupported_legacy_image_size() {
        let request = OpenAiCreateImageRequest {
            body: cireq::RequestBody {
                prompt: "A city skyline".to_string(),
                size: Some(it::OpenAiImageSize::S256x256),
                ..cireq::RequestBody::default()
            },
            ..OpenAiCreateImageRequest::default()
        };

        let error = OpenAiCreateResponseRequest::try_from(request).expect_err("expected failure");
        assert_eq!(
            error,
            TransformError::not_implemented(
                "cannot convert OpenAI image request with unsupported size to OpenAI responses.create request",
            )
        );
    }
}
