use std::collections::HashSet;

use crate::openai::create_image_edit::stream::{
    ImageEditStreamEvent, OpenAiCreateImageEditSseData, OpenAiCreateImageEditSseEvent,
    OpenAiCreateImageEditSseStreamBody,
};
use crate::openai::create_response::stream::{
    OpenAiCreateResponseSseData, OpenAiCreateResponseSseStreamBody, ResponseStreamEvent,
};
use crate::transform::openai::create_image::utils::{
    PreferredImageAction, best_effort_image_usage_from_response_usage,
    image_stream_context_from_response_stream, stream_background_from_response_config,
    stream_error_from_response_error, stream_output_format_from_response_config,
    stream_quality_from_response_config_for_create_image_edit,
    stream_size_from_response_config_for_create_image_edit,
};
use crate::transform::utils::TransformError;

fn partial_image_event(
    event: Option<String>,
    ctx: &crate::transform::openai::create_image::utils::ImageStreamContext,
    b64_json: String,
    partial_image_index: u64,
) -> OpenAiCreateImageEditSseEvent {
    OpenAiCreateImageEditSseEvent {
        event,
        data: OpenAiCreateImageEditSseData::Event(ImageEditStreamEvent::PartialImage {
            b64_json,
            background: stream_background_from_response_config(ctx.background.as_ref()),
            created_at: ctx.created_at.unwrap_or_default(),
            output_format: stream_output_format_from_response_config(ctx.output_format.as_ref()),
            partial_image_index: partial_image_index.min(u32::MAX as u64) as u32,
            quality: stream_quality_from_response_config_for_create_image_edit(
                ctx.quality.as_ref(),
            ),
            size: stream_size_from_response_config_for_create_image_edit(ctx.size.as_ref()),
        }),
    }
}

fn completed_image_event(
    event: Option<String>,
    ctx: &crate::transform::openai::create_image::utils::ImageStreamContext,
    b64_json: String,
    usage: Option<&crate::openai::create_response::types::ResponseUsage>,
) -> OpenAiCreateImageEditSseEvent {
    OpenAiCreateImageEditSseEvent {
        event,
        data: OpenAiCreateImageEditSseData::Event(ImageEditStreamEvent::Completed {
            b64_json,
            background: stream_background_from_response_config(ctx.background.as_ref()),
            created_at: ctx.created_at.unwrap_or_default(),
            output_format: stream_output_format_from_response_config(ctx.output_format.as_ref()),
            quality: stream_quality_from_response_config_for_create_image_edit(
                ctx.quality.as_ref(),
            ),
            size: stream_size_from_response_config_for_create_image_edit(ctx.size.as_ref()),
            usage: best_effort_image_usage_from_response_usage(usage.or(ctx.usage.as_ref())),
        }),
    }
}

impl TryFrom<OpenAiCreateResponseSseStreamBody> for OpenAiCreateImageEditSseStreamBody {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseSseStreamBody) -> Result<Self, TransformError> {
        let ctx = image_stream_context_from_response_stream(&value, PreferredImageAction::Edit);
        let mut events = Vec::new();
        let mut emitted_completed = HashSet::new();

        for sse_event in value.events {
            match sse_event.data {
                OpenAiCreateResponseSseData::Done(marker) => {
                    events.push(OpenAiCreateImageEditSseEvent {
                        event: sse_event.event,
                        data: OpenAiCreateImageEditSseData::Done(marker),
                    });
                }
                OpenAiCreateResponseSseData::Event(event) => match event {
                    ResponseStreamEvent::Error {
                        code,
                        message,
                        param,
                        ..
                    } => events.push(OpenAiCreateImageEditSseEvent {
                        event: sse_event.event,
                        data: OpenAiCreateImageEditSseData::Event(ImageEditStreamEvent::Error {
                            error: stream_error_from_response_error(code, message, param),
                        }),
                    }),
                    ResponseStreamEvent::ImageGenerationCallPartialImage {
                        partial_image_b64,
                        partial_image_index,
                        ..
                    } => events.push(partial_image_event(
                        sse_event.event,
                        &ctx,
                        partial_image_b64,
                        partial_image_index,
                    )),
                    ResponseStreamEvent::ImageGenerationCallCompleted { item_id, .. } => {
                        if emitted_completed.insert(item_id.clone()) {
                            if let Some(result) = ctx.results_by_item_id.get(&item_id) {
                                events.push(completed_image_event(
                                    sse_event.event,
                                    &ctx,
                                    result.clone(),
                                    ctx.usage.as_ref(),
                                ));
                            }
                        }
                    }
                    ResponseStreamEvent::OutputItemDone { item, .. } => {
                        if let crate::openai::create_response::types::ResponseOutputItem::ImageGenerationCall(call) = item {
                            if !ctx.explicit_completed_item_ids.contains(&call.id)
                                && emitted_completed.insert(call.id.clone())
                                && !call.result.is_empty()
                            {
                                events.push(completed_image_event(
                                    sse_event.event,
                                    &ctx,
                                    call.result,
                                    ctx.usage.as_ref(),
                                ));
                            }
                        }
                    }
                    ResponseStreamEvent::Completed { response, .. } => {
                        for item in response.output {
                            let crate::openai::create_response::types::ResponseOutputItem::ImageGenerationCall(call) = item else {
                                continue;
                            };
                            if ctx.explicit_completed_item_ids.contains(&call.id)
                                || !emitted_completed.insert(call.id.clone())
                                || call.result.is_empty()
                            {
                                continue;
                            }
                            events.push(completed_image_event(
                                sse_event.event.clone(),
                                &ctx,
                                call.result,
                                response.usage.as_ref(),
                            ));
                        }
                    }
                    _ => {}
                },
            }
        }

        Ok(OpenAiCreateImageEditSseStreamBody { events })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::count_tokens::types as ot;
    use crate::openai::create_response::stream::OpenAiCreateResponseSseEvent;
    use crate::transform::openai::stream_generate_content::openai_response::utils::{
        response_snapshot, response_usage_from_counts,
    };

    fn image_tool(
        action: ot::ResponseImageGenerationAction,
    ) -> crate::openai::create_response::types::ResponseTool {
        crate::openai::create_response::types::ResponseTool::ImageGeneration(
            ot::ResponseImageGenerationTool {
                type_: ot::ResponseImageGenerationToolType::ImageGeneration,
                action: Some(action),
                background: Some(ot::ResponseImageGenerationBackground::Opaque),
                input_fidelity: Some(ot::ResponseImageGenerationInputFidelity::High),
                input_image_mask: None,
                model: Some(ot::ResponseImageGenerationModel::Known(
                    ot::ResponseImageGenerationModelKnown::GptImage1,
                )),
                moderation: Some(ot::ResponseImageGenerationModeration::Auto),
                output_compression: Some(100),
                output_format: Some(ot::ResponseImageGenerationOutputFormat::Webp),
                partial_images: Some(1),
                quality: Some(ot::ResponseImageGenerationQuality::Medium),
                size: Some(ot::ResponseImageGenerationSize::S1024x1536),
            },
        )
    }

    #[test]
    fn converts_response_stream_to_create_image_edit_stream() {
        let mut created = response_snapshot(
            "resp_1",
            "gpt-image-1",
            Some(crate::openai::create_response::types::ResponseStatus::InProgress),
            None,
            None,
            None,
            None,
        );
        created.created_at = 1_741_383_474;
        created.tools = vec![image_tool(ot::ResponseImageGenerationAction::Edit)];

        let mut completed = created.clone();
        completed.status = Some(crate::openai::create_response::types::ResponseStatus::Completed);
        completed.usage = Some(response_usage_from_counts(300, 0, 50, 0));
        completed.output = vec![
            crate::openai::create_response::types::ResponseOutputItem::ImageGenerationCall(
                ot::ResponseImageGenerationCall {
                    id: "igc_1".to_string(),
                    result: "edit-final-b64".to_string(),
                    status: ot::ResponseImageGenerationCallStatus::Completed,
                    type_: ot::ResponseImageGenerationCallType::ImageGenerationCall,
                },
            ),
        ];

        let stream = OpenAiCreateResponseSseStreamBody {
            events: vec![
                OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Event(ResponseStreamEvent::Created {
                        response: created,
                        sequence_number: 0,
                    }),
                },
                OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Event(
                        ResponseStreamEvent::ImageGenerationCallPartialImage {
                            item_id: "igc_1".to_string(),
                            output_index: 0,
                            partial_image_b64: "edit-partial-b64".to_string(),
                            partial_image_index: 0,
                            sequence_number: 1,
                        },
                    ),
                },
                OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Event(
                        ResponseStreamEvent::ImageGenerationCallCompleted {
                            item_id: "igc_1".to_string(),
                            output_index: 0,
                            sequence_number: 2,
                        },
                    ),
                },
                OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Event(ResponseStreamEvent::Completed {
                        response: completed,
                        sequence_number: 3,
                    }),
                },
                OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Done("[DONE]".to_string()),
                },
            ],
        };

        let converted = OpenAiCreateImageEditSseStreamBody::try_from(stream).unwrap();
        assert_eq!(converted.events.len(), 3);

        match &converted.events[0].data {
            OpenAiCreateImageEditSseData::Event(ImageEditStreamEvent::PartialImage {
                b64_json,
                created_at,
                partial_image_index,
                ..
            }) => {
                assert_eq!(b64_json, "edit-partial-b64");
                assert_eq!(*created_at, 1_741_383_474);
                assert_eq!(*partial_image_index, 0);
            }
            other => panic!("unexpected first event: {other:?}"),
        }

        match &converted.events[1].data {
            OpenAiCreateImageEditSseData::Event(ImageEditStreamEvent::Completed {
                b64_json,
                usage,
                ..
            }) => {
                assert_eq!(b64_json, "edit-final-b64");
                assert_eq!(usage.total_tokens, 350);
            }
            other => panic!("unexpected second event: {other:?}"),
        }
    }

    #[test]
    fn maps_response_error_to_image_edit_error_event() {
        let stream = OpenAiCreateResponseSseStreamBody {
            events: vec![
                OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Event(ResponseStreamEvent::Error {
                        code: "invalid_image".to_string(),
                        message: "bad image".to_string(),
                        param: Some("image".to_string()),
                        sequence_number: 0,
                    }),
                },
                OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Done("[DONE]".to_string()),
                },
            ],
        };

        let converted = OpenAiCreateImageEditSseStreamBody::try_from(stream).unwrap();
        assert_eq!(converted.events.len(), 2);
        match &converted.events[0].data {
            OpenAiCreateImageEditSseData::Event(ImageEditStreamEvent::Error { error }) => {
                assert_eq!(error.code.as_deref(), Some("invalid_image"));
                assert_eq!(error.param.as_deref(), Some("image"));
            }
            other => panic!("unexpected first event: {other:?}"),
        }
    }
}
