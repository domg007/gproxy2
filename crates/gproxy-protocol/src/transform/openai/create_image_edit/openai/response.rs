use crate::openai::create_image_edit::response::OpenAiCreateImageEditResponse;
use crate::openai::create_response::response::OpenAiCreateResponseResponse;
use crate::transform::openai::create_image::utils::{
    PreferredImageAction, create_image_response_body_from_response,
};
use crate::transform::utils::TransformError;

impl TryFrom<OpenAiCreateResponseResponse> for OpenAiCreateImageEditResponse {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseResponse) -> Result<Self, TransformError> {
        Ok(match value {
            OpenAiCreateResponseResponse::Success {
                stats_code,
                headers,
                body,
            } => OpenAiCreateImageEditResponse::Success {
                stats_code,
                headers,
                body: create_image_response_body_from_response(body, PreferredImageAction::Edit)?,
            },
            OpenAiCreateResponseResponse::Error {
                stats_code,
                headers,
                body,
            } => OpenAiCreateImageEditResponse::Error {
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
    use crate::openai::create_image::types as it;
    use crate::openai::create_response::response::ResponseBody;
    use crate::openai::create_response::types as rt;
    use crate::openai::types::OpenAiResponseHeaders;

    fn sample_headers() -> OpenAiResponseHeaders {
        OpenAiResponseHeaders::default()
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

    #[test]
    fn converts_openai_response_to_create_image_edit_response() {
        let converted =
            OpenAiCreateImageEditResponse::try_from(OpenAiCreateResponseResponse::Success {
                stats_code: StatusCode::OK,
                headers: sample_headers(),
                body: sample_response_body(ot::ResponseImageGenerationAction::Edit),
            })
            .unwrap();

        let OpenAiCreateImageEditResponse::Success { body, .. } = converted else {
            panic!("expected success response");
        };

        assert_eq!(body.created, 1_741_383_474);
        assert_eq!(body.output_format, Some(it::OpenAiImageOutputFormat::Png));
        assert_eq!(body.quality, Some(it::OpenAiGeneratedImageQuality::High));
        assert_eq!(body.size, Some(it::OpenAiGeneratedImageSize::S1024x1024));
        assert_eq!(
            body.data.unwrap()[0].b64_json.as_deref(),
            Some("base64-image")
        );
    }
}
