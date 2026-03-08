use crate::openai::create_image::response::OpenAiCreateImageResponse;
use crate::openai::create_image::types as it;
use crate::openai::create_response::response::{OpenAiCreateResponseResponse, ResponseBody};
use crate::transform::openai::create_image::utils::{
    PreferredImageAction, create_image_response_body_from_response,
};
use crate::transform::utils::TransformError;

impl TryFrom<ResponseBody> for it::OpenAiCreateImageResponseBody {
    type Error = TransformError;

    fn try_from(value: ResponseBody) -> Result<Self, TransformError> {
        create_image_response_body_from_response(value, PreferredImageAction::Generate)
    }
}

impl TryFrom<OpenAiCreateResponseResponse> for OpenAiCreateImageResponse {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseResponse) -> Result<Self, TransformError> {
        Ok(match value {
            OpenAiCreateResponseResponse::Success {
                stats_code,
                headers,
                body,
            } => OpenAiCreateImageResponse::Success {
                stats_code,
                headers,
                body: it::OpenAiCreateImageResponseBody::try_from(body)?,
            },
            OpenAiCreateResponseResponse::Error {
                stats_code,
                headers,
                body,
            } => OpenAiCreateImageResponse::Error {
                stats_code,
                headers,
                body,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use http::StatusCode;

    use super::*;
    use crate::openai::count_tokens::types as ot;
    use crate::openai::create_response::types as rt;
    use crate::openai::types::{OpenAiApiError, OpenAiApiErrorResponse, OpenAiResponseHeaders};

    fn sample_headers() -> OpenAiResponseHeaders {
        OpenAiResponseHeaders::default()
    }

    fn sample_error_body() -> OpenAiApiErrorResponse {
        OpenAiApiErrorResponse {
            error: OpenAiApiError {
                message: "boom".to_string(),
                type_: "invalid_request_error".to_string(),
                param: Some("input".to_string()),
                code: Some("invalid_image".to_string()),
            },
        }
    }

    fn sample_image_tool(action: ot::ResponseImageGenerationAction) -> rt::ResponseTool {
        rt::ResponseTool::ImageGeneration(ot::ResponseImageGenerationTool {
            type_: ot::ResponseImageGenerationToolType::ImageGeneration,
            action: Some(action),
            background: Some(ot::ResponseImageGenerationBackground::Transparent),
            input_fidelity: None,
            input_image_mask: None,
            model: Some(ot::ResponseImageGenerationModel::Known(
                ot::ResponseImageGenerationModelKnown::GptImage1,
            )),
            moderation: Some(ot::ResponseImageGenerationModeration::Low),
            output_compression: Some(100),
            output_format: Some(ot::ResponseImageGenerationOutputFormat::Png),
            partial_images: Some(1),
            quality: Some(ot::ResponseImageGenerationQuality::High),
            size: Some(ot::ResponseImageGenerationSize::S1024x1024),
        })
    }

    fn sample_response_body(action: ot::ResponseImageGenerationAction) -> ResponseBody {
        ResponseBody {
            id: "resp_123".to_string(),
            created_at: 1_741_383_474,
            error: None,
            incomplete_details: None,
            instructions: None,
            metadata: BTreeMap::new(),
            model: "gpt-image-1".to_string(),
            object: rt::ResponseObject::Response,
            output: vec![rt::ResponseOutputItem::ImageGenerationCall(
                ot::ResponseImageGenerationCall {
                    id: "igc_123".to_string(),
                    result: "base64-image".to_string(),
                    status: ot::ResponseImageGenerationCallStatus::Completed,
                    type_: ot::ResponseImageGenerationCallType::ImageGenerationCall,
                },
            )],
            parallel_tool_calls: false,
            temperature: 1.0,
            tool_choice: rt::ResponseToolChoice::Options(ot::ResponseToolChoiceOptions::Auto),
            tools: vec![sample_image_tool(action)],
            top_p: 1.0,
            background: None,
            completed_at: Some(1_741_383_475),
            conversation: None,
            max_output_tokens: None,
            max_tool_calls: None,
            output_text: None,
            previous_response_id: None,
            prompt: None,
            prompt_cache_key: None,
            prompt_cache_retention: None,
            reasoning: None,
            safety_identifier: None,
            service_tier: None,
            status: Some(rt::ResponseStatus::Completed),
            text: None,
            top_logprobs: None,
            truncation: None,
            usage: None,
            user: None,
        }
    }

    fn sample_non_image_response_body() -> ResponseBody {
        ResponseBody {
            id: "resp_456".to_string(),
            created_at: 1,
            error: None,
            incomplete_details: None,
            instructions: None,
            metadata: BTreeMap::new(),
            model: "gpt-4.1".to_string(),
            object: rt::ResponseObject::Response,
            output: vec![rt::ResponseOutputItem::Message(ot::ResponseOutputMessage {
                id: "msg_123".to_string(),
                content: vec![ot::ResponseOutputContent::Text(ot::ResponseOutputText {
                    annotations: Vec::new(),
                    logprobs: None,
                    text: "hello".to_string(),
                    type_: ot::ResponseOutputTextType::OutputText,
                })],
                role: ot::ResponseOutputMessageRole::Assistant,
                phase: None,
                status: ot::ResponseItemStatus::Completed,
                type_: ot::ResponseOutputMessageType::Message,
            })],
            parallel_tool_calls: false,
            temperature: 1.0,
            tool_choice: rt::ResponseToolChoice::Options(ot::ResponseToolChoiceOptions::Auto),
            tools: Vec::new(),
            top_p: 1.0,
            background: None,
            completed_at: None,
            conversation: None,
            max_output_tokens: None,
            max_tool_calls: None,
            output_text: Some("hello".to_string()),
            previous_response_id: None,
            prompt: None,
            prompt_cache_key: None,
            prompt_cache_retention: None,
            reasoning: None,
            safety_identifier: None,
            service_tier: None,
            status: Some(rt::ResponseStatus::Completed),
            text: None,
            top_logprobs: None,
            truncation: None,
            usage: None,
            user: None,
        }
    }

    #[test]
    fn converts_openai_response_to_create_image_response() {
        let converted =
            OpenAiCreateImageResponse::try_from(OpenAiCreateResponseResponse::Success {
                stats_code: StatusCode::OK,
                headers: sample_headers(),
                body: sample_response_body(ot::ResponseImageGenerationAction::Generate),
            })
            .unwrap();

        let OpenAiCreateImageResponse::Success { body, .. } = converted else {
            panic!("expected success response");
        };

        assert_eq!(body.created, 1_741_383_474);
        assert_eq!(
            body.background,
            Some(it::OpenAiGeneratedImageBackground::Transparent)
        );
        assert_eq!(body.output_format, Some(it::OpenAiImageOutputFormat::Png));
        assert_eq!(body.quality, Some(it::OpenAiGeneratedImageQuality::High));
        assert_eq!(body.size, Some(it::OpenAiGeneratedImageSize::S1024x1024));
        assert_eq!(
            body.data.unwrap()[0].b64_json.as_deref(),
            Some("base64-image")
        );
        assert!(body.usage.is_none());
    }

    #[test]
    fn passes_through_openai_error_response() {
        let converted = OpenAiCreateImageResponse::try_from(OpenAiCreateResponseResponse::Error {
            stats_code: StatusCode::BAD_REQUEST,
            headers: sample_headers(),
            body: sample_error_body(),
        })
        .unwrap();

        let OpenAiCreateImageResponse::Error {
            stats_code, body, ..
        } = converted
        else {
            panic!("expected error response");
        };

        assert_eq!(stats_code, StatusCode::BAD_REQUEST);
        assert_eq!(body.error.message, "boom");
        assert_eq!(body.error.param.as_deref(), Some("input"));
    }

    #[test]
    fn rejects_non_image_response_body() {
        let error = it::OpenAiCreateImageResponseBody::try_from(sample_non_image_response_body())
            .expect_err("expected conversion failure");

        assert_eq!(
            error,
            TransformError::not_implemented(
                "cannot convert OpenAI response without image_generation_call",
            )
        );
    }
}
