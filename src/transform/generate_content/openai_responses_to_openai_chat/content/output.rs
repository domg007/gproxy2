use crate::protocol::openai;

use crate::transform::generate_content::openai_responses_to_openai_chat::tools::{
    custom_call_to_chat_tool_call, function_call_to_chat_tool_call,
};

pub(in crate::transform::generate_content::openai_responses_to_openai_chat) fn response_output_items_to_chat_message(
    items: Vec<openai::ResponseOutputItem>,
    fallback_text: Option<String>,
) -> openai::ChatMessage {
    let mut text_parts = Vec::new();
    let mut reasoning_parts = Vec::new();
    let mut reasoning_details = Vec::new();
    let mut refusal = None;
    let mut annotations = Vec::new();
    let mut tool_calls = Vec::new();

    for item in items {
        match item.0 {
            openai::ResponseItem::Message(openai::ResponseMessageItem::Output(message)) => {
                for part in message.content {
                    match part {
                        openai::ResponseMessageOutputContentPart::OutputText {
                            text,
                            annotations: part_annotations,
                            ..
                        } => {
                            text_parts.push(text);
                            annotations.extend(
                                part_annotations
                                    .into_iter()
                                    .filter_map(response_annotation_to_chat_annotation),
                            );
                        }
                        openai::ResponseMessageOutputContentPart::Refusal {
                            refusal: value,
                            ..
                        } => {
                            refusal = Some(value.clone());
                            text_parts.push(value);
                        }
                    }
                }
            }
            openai::ResponseItem::Typed(openai::TypedResponseItem::FunctionCall {
                arguments,
                call_id,
                name,
                ..
            }) => {
                tool_calls.push(function_call_to_chat_tool_call(call_id, name, arguments));
            }
            openai::ResponseItem::Typed(openai::TypedResponseItem::CustomToolCall {
                call_id,
                input,
                name,
                ..
            }) => {
                tool_calls.push(custom_call_to_chat_tool_call(call_id, name, input));
            }
            openai::ResponseItem::Typed(
                reasoning @ openai::TypedResponseItem::Reasoning { .. },
            ) => {
                append_reasoning_item(reasoning, &mut reasoning_parts, &mut reasoning_details);
            }
            _ => {}
        }
    }

    if text_parts.is_empty()
        && let Some(text) = fallback_text.filter(|value| !value.is_empty())
    {
        text_parts.push(text);
    }

    openai::ChatMessage {
        role: openai::ChatCompletionMessageRole::Assistant,
        content: (!text_parts.is_empty()).then(|| text_parts.join("\n")),
        reasoning_content: (!reasoning_parts.is_empty()).then(|| reasoning_parts.join("\n")),
        reasoning_details: (!reasoning_details.is_empty()).then_some(reasoning_details),
        refusal,
        annotations: (!annotations.is_empty()).then_some(annotations),
        audio: None,
        function_call: None,
        tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
        extra: Default::default(),
    }
}

fn append_reasoning_item(
    item: openai::TypedResponseItem,
    reasoning_parts: &mut Vec<String>,
    reasoning_details: &mut Vec<openai::ChatReasoningDetail>,
) {
    let openai::TypedResponseItem::Reasoning {
        id,
        summary,
        content,
        encrypted_content,
        signature,
        ..
    } = item
    else {
        return;
    };
    let mut index = reasoning_details.len() as u64;

    if let Some(content) = content {
        for part in content {
            if part.text.is_empty() {
                continue;
            }
            reasoning_parts.push(part.text.clone());
            reasoning_details.push(openai::ChatReasoningDetail {
                type_: openai::ChatReasoningDetailType::Text,
                id: Some(id.clone()),
                data: None,
                text: Some(part.text),
                signature: signature.clone(),
                index: Some(index),
            });
            index += 1;
        }
    }

    for part in summary {
        if part.text.is_empty() {
            continue;
        }
        reasoning_details.push(openai::ChatReasoningDetail {
            type_: openai::ChatReasoningDetailType::Summary,
            id: Some(id.clone()),
            data: None,
            text: Some(part.text),
            signature: signature.clone(),
            index: Some(index),
        });
        index += 1;
    }

    if let Some(data) = encrypted_content.filter(|value| !value.is_empty()) {
        reasoning_details.push(openai::ChatReasoningDetail {
            type_: openai::ChatReasoningDetailType::Encrypted,
            id: Some(id),
            data: Some(data),
            text: None,
            signature,
            index: Some(index),
        });
    }
}

fn response_annotation_to_chat_annotation(
    annotation: openai::ResponseAnnotation,
) -> Option<openai::ChatAnnotation> {
    match annotation {
        openai::ResponseAnnotation::UrlCitation {
            end_index,
            start_index,
            title,
            url,
            ..
        } => Some(openai::ChatAnnotation {
            type_: openai::ChatAnnotationType::UrlCitation,
            url_citation: openai::UrlCitation {
                end_index,
                start_index,
                title,
                url,
                extra: Default::default(),
            },
            extra: Default::default(),
        }),
        _ => None,
    }
}
