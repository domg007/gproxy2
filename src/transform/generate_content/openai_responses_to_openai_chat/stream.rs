use crate::protocol::openai;
use crate::transform::{TransformContext, TransformError};

use super::super::common;
use super::usage::response_usage_to_chat;

pub fn stream_event(
    input: openai::ResponseStreamEvent,
    _: &TransformContext,
) -> Result<openai::ChatCompletionChunk, TransformError> {
    Ok(match input {
        openai::ResponseStreamEvent::Known(event) => known_event_to_chat(event),
        openai::ResponseStreamEvent::Unknown(_) => common::empty_chat_chunk(
            "resp_unknown".to_owned(),
            common::default_openai_model(),
            0,
            None,
        ),
    })
}

fn known_event_to_chat(event: openai::KnownResponseStreamEvent) -> openai::ChatCompletionChunk {
    match event {
        openai::KnownResponseStreamEvent::ResponseCreated { response, .. }
        | openai::KnownResponseStreamEvent::ResponseInProgress { response, .. }
        | openai::KnownResponseStreamEvent::ResponseQueued { response, .. } => {
            empty_from_response(*response)
        }
        openai::KnownResponseStreamEvent::ResponseOutputItemAdded {
            item, output_index, ..
        } => output_item_added_to_chat(*item, output_index),
        openai::KnownResponseStreamEvent::ResponseCompleted { response, .. }
        | openai::KnownResponseStreamEvent::ResponseFailed { response, .. }
        | openai::KnownResponseStreamEvent::ResponseIncomplete { response, .. } => {
            finish_from_response(*response)
        }
        openai::KnownResponseStreamEvent::ResponseOutputTextDelta {
            delta,
            item_id,
            output_index: _,
            ..
        } => common::chat_text_delta(item_id, common::default_openai_model(), 0, 0, delta),
        openai::KnownResponseStreamEvent::ResponseAudioTranscriptDelta { delta, .. } => {
            common::chat_text_delta(
                "resp_audio_transcript".to_owned(),
                common::default_openai_model(),
                0,
                0,
                delta,
            )
        }
        openai::KnownResponseStreamEvent::ResponseRefusalDelta {
            delta,
            item_id,
            output_index: _,
            ..
        } => common::chat_refusal_delta(item_id, common::default_openai_model(), 0, 0, delta),
        openai::KnownResponseStreamEvent::ResponseReasoningSummaryTextDelta {
            delta,
            item_id,
            output_index: _,
            ..
        }
        | openai::KnownResponseStreamEvent::ResponseReasoningTextDelta {
            delta,
            item_id,
            output_index: _,
            ..
        } => common::chat_reasoning_delta(item_id, common::default_openai_model(), 0, 0, delta),
        openai::KnownResponseStreamEvent::ResponseFunctionCallArgumentsDelta {
            delta,
            item_id,
            output_index,
            ..
        } => {
            let call_id = common::fallback_response_call_id(output_index, Some(&item_id));
            chat_tool_delta(
                call_id.clone(),
                output_index,
                common::chat_function_tool_delta(output_index, Some(call_id), None, Some(delta)),
                None,
            )
        }
        openai::KnownResponseStreamEvent::ResponseFunctionCallArgumentsDone {
            arguments: _,
            item_id,
            name,
            output_index,
            ..
        } => {
            let call_id = common::fallback_response_call_id(output_index, Some(&item_id));
            chat_tool_delta(
                call_id.clone(),
                output_index,
                common::chat_function_tool_delta(output_index, Some(call_id), Some(name), None),
                None,
            )
        }
        openai::KnownResponseStreamEvent::ResponseCustomToolCallInputDelta {
            delta,
            item_id,
            output_index,
            ..
        } => {
            let call_id = common::fallback_response_call_id(output_index, Some(&item_id));
            chat_tool_delta(
                call_id.clone(),
                output_index,
                common::chat_custom_tool_delta(output_index, Some(call_id), None, Some(delta)),
                None,
            )
        }
        openai::KnownResponseStreamEvent::ResponseCustomToolCallInputDone {
            input: _,
            item_id,
            output_index,
            ..
        } => {
            let call_id = common::fallback_response_call_id(output_index, Some(&item_id));
            chat_tool_delta(
                call_id.clone(),
                output_index,
                common::chat_custom_tool_delta(output_index, Some(call_id), None, None),
                None,
            )
        }
        openai::KnownResponseStreamEvent::Error { .. } => common::chat_finish_chunk(
            "resp_error".to_owned(),
            common::default_openai_model(),
            0,
            openai::ChatFinishReason::ContentFilter,
            None,
        ),
        _ => common::empty_chat_chunk(
            "resp_event".to_owned(),
            common::default_openai_model(),
            0,
            None,
        ),
    }
}

fn chat_tool_delta(
    id: String,
    _index: u32,
    tool_delta: openai::ChatToolCallDelta,
    finish_reason: Option<openai::ChatFinishReason>,
) -> openai::ChatCompletionChunk {
    let mut delta = common::empty_chat_delta();
    delta.tool_calls = Some(vec![tool_delta]);
    common::chat_delta_chunk(
        id,
        common::default_openai_model(),
        0,
        0,
        delta,
        finish_reason,
        None,
    )
}

fn output_item_added_to_chat(
    item: openai::ResponseOutputItem,
    output_index: u32,
) -> openai::ChatCompletionChunk {
    match item.0 {
        openai::ResponseItem::Typed(openai::TypedResponseItem::FunctionCall {
            call_id,
            name,
            ..
        }) => chat_tool_delta(
            call_id.clone(),
            output_index,
            common::chat_function_tool_delta(output_index, Some(call_id), Some(name), None),
            None,
        ),
        openai::ResponseItem::Typed(openai::TypedResponseItem::CustomToolCall {
            call_id,
            name,
            ..
        }) => chat_tool_delta(
            call_id.clone(),
            output_index,
            common::chat_custom_tool_delta(output_index, Some(call_id), Some(name), None),
            None,
        ),
        _ => common::empty_chat_chunk(
            "resp_output_item".to_owned(),
            common::default_openai_model(),
            0,
            None,
        ),
    }
}

fn empty_from_response(response: openai::ResponseObject) -> openai::ChatCompletionChunk {
    let id = response.id;
    let created = response.created_at;
    let model = response.model.unwrap_or_else(common::default_openai_model);
    let usage = response_usage_to_chat(response.usage);
    common::empty_chat_chunk(id, model, created, usage)
}

fn finish_from_response(response: openai::ResponseObject) -> openai::ChatCompletionChunk {
    let finish_reason = response_finish_reason(&response);
    let id = response.id;
    let created = response.created_at;
    let model = response.model.unwrap_or_else(common::default_openai_model);
    let usage = response_usage_to_chat(response.usage);
    common::chat_finish_chunk(id, model, created, finish_reason, usage)
}

fn response_finish_reason(response: &openai::ResponseObject) -> openai::ChatFinishReason {
    if response.output.iter().any(|item| {
        matches!(
            &item.0,
            openai::ResponseItem::Typed(
                openai::TypedResponseItem::FunctionCall { .. }
                    | openai::TypedResponseItem::CustomToolCall { .. }
            )
        )
    }) {
        return openai::ChatFinishReason::ToolCalls;
    }

    match response.status {
        Some(openai::ResponseStatus::Incomplete) => response
            .incomplete_details
            .as_ref()
            .and_then(|details| details.reason.as_ref())
            .map(incomplete_reason_to_chat)
            .unwrap_or(openai::ChatFinishReason::Length),
        Some(openai::ResponseStatus::Failed | openai::ResponseStatus::Cancelled) => {
            openai::ChatFinishReason::ContentFilter
        }
        _ => openai::ChatFinishReason::Stop,
    }
}

fn incomplete_reason_to_chat(reason: &openai::IncompleteReason) -> openai::ChatFinishReason {
    match reason {
        openai::IncompleteReason::MaxOutputTokens => openai::ChatFinishReason::Length,
        openai::IncompleteReason::ContentFilter => openai::ChatFinishReason::ContentFilter,
    }
}
