use crate::protocol::openai;
use crate::transform::{TransformContext, TransformError};

use super::super::common;
use super::usage::chat_usage_to_response;

pub fn stream_event(
    input: openai::ChatCompletionChunk,
    _: &TransformContext,
) -> Result<openai::ResponseStreamEvent, TransformError> {
    Ok(chat_chunk_to_response_event(input))
}

fn chat_chunk_to_response_event(input: openai::ChatCompletionChunk) -> openai::ResponseStreamEvent {
    let id = input.id;
    let created = input.created;
    let model = input.model;
    let usage = input.usage;

    let Some(choice) = input.choices.into_iter().next() else {
        return response_lifecycle_event(
            id,
            created,
            model,
            usage,
            openai::ResponseStatus::Completed,
            None,
        );
    };

    let index = choice.index;
    let finish_reason = choice.finish_reason;
    let delta = choice.delta;

    if let Some(content) = delta.content.filter(|value| !value.is_empty()) {
        return known(openai::KnownResponseStreamEvent::ResponseOutputTextDelta {
            content_index: 0,
            delta: content,
            item_id: message_item_id(index),
            logprobs: None,
            output_index: index,
            sequence_number: None,
            extra: Default::default(),
        });
    }

    if let Some(reasoning) = delta.reasoning_content.filter(|value| !value.is_empty()) {
        return known(
            openai::KnownResponseStreamEvent::ResponseReasoningTextDelta {
                content_index: 0,
                delta: reasoning,
                item_id: reasoning_item_id(index),
                output_index: index,
                sequence_number: None,
                extra: Default::default(),
            },
        );
    }

    if let Some(refusal) = delta.refusal.filter(|value| !value.is_empty()) {
        return known(openai::KnownResponseStreamEvent::ResponseRefusalDelta {
            content_index: 0,
            delta: refusal,
            item_id: message_item_id(index),
            output_index: index,
            sequence_number: None,
            extra: Default::default(),
        });
    }

    if let Some(event) = delta
        .tool_calls
        .and_then(|tool_calls| tool_calls.into_iter().next())
        .map(tool_delta_to_response_event)
    {
        return event;
    }

    if let Some(function_call) = delta.function_call {
        if let Some(arguments) = function_call.arguments.filter(|value| !value.is_empty()) {
            return known(
                openai::KnownResponseStreamEvent::ResponseFunctionCallArgumentsDelta {
                    delta: arguments,
                    item_id: common::indexed_response_function_call_item_id(index),
                    output_index: index,
                    sequence_number: None,
                    extra: Default::default(),
                },
            );
        }
        if let Some(name) = function_call.name.filter(|value| !value.is_empty()) {
            let call_id = common::indexed_response_call_id(index);
            return output_item_added(
                index,
                openai::ResponseItem::Typed(openai::TypedResponseItem::FunctionCall {
                    arguments: String::new(),
                    call_id,
                    name,
                    id: Some(common::indexed_response_function_call_item_id(index)),
                    namespace: None,
                    status: Some(openai::ResponseItemLifecycleStatus::InProgress),
                    extra: Default::default(),
                }),
            );
        }
    }

    if let Some(reason) = finish_reason {
        let (status, incomplete) = response_status_from_finish(reason);
        return response_lifecycle_event(id, created, model, usage, status, incomplete);
    }

    response_lifecycle_event(
        id,
        created,
        model,
        usage,
        openai::ResponseStatus::InProgress,
        None,
    )
}

fn tool_delta_to_response_event(call: openai::ChatToolCallDelta) -> openai::ResponseStreamEvent {
    let output_index = call.index;
    if let Some(function) = call.function {
        let (call_id, item_id) = response_function_ids(call.id.as_deref(), output_index);
        if let Some(arguments) = function.arguments.filter(|value| !value.is_empty()) {
            return known(
                openai::KnownResponseStreamEvent::ResponseFunctionCallArgumentsDelta {
                    delta: arguments,
                    item_id,
                    output_index,
                    sequence_number: None,
                    extra: Default::default(),
                },
            );
        }
        if let Some(name) = function.name.filter(|value| !value.is_empty()) {
            return output_item_added(
                output_index,
                openai::ResponseItem::Typed(openai::TypedResponseItem::FunctionCall {
                    arguments: String::new(),
                    call_id,
                    name,
                    id: Some(item_id),
                    namespace: None,
                    status: Some(openai::ResponseItemLifecycleStatus::InProgress),
                    extra: Default::default(),
                }),
            );
        }
    }

    if let Some(custom) = call.custom {
        let call_id = call
            .id
            .as_deref()
            .map(common::response_call_id)
            .unwrap_or_else(|| common::indexed_response_call_id(output_index));
        let item_id = format!("ctc_{output_index}");
        if let Some(input) = custom.input.filter(|value| !value.is_empty()) {
            return known(
                openai::KnownResponseStreamEvent::ResponseCustomToolCallInputDelta {
                    delta: input,
                    item_id,
                    output_index,
                    sequence_number: None,
                    extra: Default::default(),
                },
            );
        }
        if let Some(name) = custom.name.filter(|value| !value.is_empty()) {
            return output_item_added(
                output_index,
                openai::ResponseItem::Typed(openai::TypedResponseItem::CustomToolCall {
                    call_id,
                    input: String::new(),
                    name,
                    id: None,
                    namespace: None,
                    extra: Default::default(),
                }),
            );
        }
    }

    response_lifecycle_event(
        "resp_tool_delta".to_owned(),
        0,
        common::default_openai_model(),
        None,
        openai::ResponseStatus::InProgress,
        None,
    )
}

fn output_item_added(output_index: u32, item: openai::ResponseItem) -> openai::ResponseStreamEvent {
    known(openai::KnownResponseStreamEvent::ResponseOutputItemAdded {
        item: Box::new(openai::ResponseOutputItem(item)),
        output_index,
        sequence_number: None,
        extra: Default::default(),
    })
}

fn response_lifecycle_event(
    id: String,
    created_at: u64,
    model: openai::OpenAiModelId,
    usage: Option<openai::CompletionUsage>,
    status: openai::ResponseStatus,
    incomplete_details: Option<openai::IncompleteDetails>,
) -> openai::ResponseStreamEvent {
    let event_status = status.clone();
    let response = Box::new(openai::ResponseObject {
        id,
        created_at,
        background: None,
        completed_at: matches!(status, openai::ResponseStatus::Completed).then_some(created_at),
        conversation: None,
        error: None,
        incomplete_details,
        instructions: None,
        max_output_tokens: None,
        max_tool_calls: None,
        metadata: None,
        model: Some(model),
        moderation: None,
        object: openai::ResponseObjectType::Response,
        output: Vec::new(),
        output_text: None,
        parallel_tool_calls: None,
        prompt: None,
        prompt_cache_key: None,
        prompt_cache_retention: None,
        previous_response_id: None,
        reasoning: None,
        safety_identifier: None,
        service_tier: None,
        status: Some(status),
        store: None,
        temperature: None,
        text: None,
        tool_choice: None,
        tools: None,
        top_logprobs: None,
        top_p: None,
        truncation: None,
        usage: chat_usage_to_response(usage),
        user: None,
        extra: Default::default(),
    });

    match event_status {
        openai::ResponseStatus::Completed => {
            known(openai::KnownResponseStreamEvent::ResponseCompleted {
                response,
                sequence_number: None,
                extra: Default::default(),
            })
        }
        openai::ResponseStatus::Incomplete => {
            known(openai::KnownResponseStreamEvent::ResponseIncomplete {
                response,
                sequence_number: None,
                extra: Default::default(),
            })
        }
        _ => known(openai::KnownResponseStreamEvent::ResponseInProgress {
            response,
            sequence_number: None,
            extra: Default::default(),
        }),
    }
}

fn response_status_from_finish(
    reason: openai::ChatFinishReason,
) -> (openai::ResponseStatus, Option<openai::IncompleteDetails>) {
    match reason {
        openai::ChatFinishReason::Length => (
            openai::ResponseStatus::Incomplete,
            Some(openai::IncompleteDetails {
                reason: Some(openai::IncompleteReason::MaxOutputTokens),
                extra: Default::default(),
            }),
        ),
        openai::ChatFinishReason::ContentFilter => (
            openai::ResponseStatus::Incomplete,
            Some(openai::IncompleteDetails {
                reason: Some(openai::IncompleteReason::ContentFilter),
                extra: Default::default(),
            }),
        ),
        _ => (openai::ResponseStatus::Completed, None),
    }
}

fn message_item_id(index: u32) -> String {
    format!("msg_{index}")
}

fn reasoning_item_id(index: u32) -> String {
    format!("reasoning_{index}")
}

fn response_function_ids(chat_call_id: Option<&str>, output_index: u32) -> (String, String) {
    chat_call_id.map_or_else(
        || {
            (
                common::indexed_response_call_id(output_index),
                common::indexed_response_function_call_item_id(output_index),
            )
        },
        |id| {
            (
                common::response_call_id(id),
                common::response_function_call_item_id(id),
            )
        },
    )
}

fn known(event: openai::KnownResponseStreamEvent) -> openai::ResponseStreamEvent {
    openai::ResponseStreamEvent::Known(event)
}
