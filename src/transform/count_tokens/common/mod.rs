use crate::protocol::{claude, gemini, openai};

pub(in crate::transform::count_tokens) const DEFAULT_MODEL: &str = "unknown";

pub(in crate::transform::count_tokens) fn u64_to_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(in crate::transform::count_tokens) fn u64_to_i32(value: u64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

pub(in crate::transform::count_tokens) fn u32_to_u64(value: u32) -> u64 {
    u64::from(value)
}

pub(in crate::transform::count_tokens) fn u32_to_i32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

pub(in crate::transform::count_tokens) fn i32_to_u32(value: i32) -> u32 {
    u32::try_from(value).unwrap_or_default()
}

pub(in crate::transform::count_tokens) fn i32_to_u64(value: i32) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

pub(in crate::transform::count_tokens) fn openai_model_string(
    model: Option<openai::OpenAiModelId>,
) -> String {
    model
        .as_ref()
        .map(model_to_string)
        .unwrap_or_else(|| DEFAULT_MODEL.to_owned())
}

pub(in crate::transform::count_tokens) fn claude_model_string(
    model: &claude::ClaudeModel,
) -> String {
    model_to_string(model)
}

pub(in crate::transform::count_tokens) fn gemini_model_string(model: Option<String>) -> String {
    model.unwrap_or_else(|| DEFAULT_MODEL.to_owned())
}

fn model_to_string<T: serde::Serialize>(model: &T) -> String {
    let Ok(value) = serde_json::to_value(model) else {
        return DEFAULT_MODEL.to_owned();
    };
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| DEFAULT_MODEL.to_owned())
}

pub(in crate::transform::count_tokens) fn openai_input_to_text(
    input: Option<openai::ResponseInput>,
) -> String {
    match input {
        Some(openai::ResponseInput::Text(text)) => text,
        Some(openai::ResponseInput::Items(items)) => items
            .into_iter()
            .map(openai_item_text)
            .collect::<Vec<_>>()
            .join("\n"),
        None => String::new(),
    }
}

fn openai_item_text(item: openai::ResponseItem) -> String {
    match item {
        openai::ResponseItem::Message(openai::ResponseMessageItem::EasyInput(message)) => {
            openai_easy_content_text(message.content)
        }
        openai::ResponseItem::Message(openai::ResponseMessageItem::Input(message)) => {
            response_input_parts_text(message.content)
        }
        openai::ResponseItem::Message(openai::ResponseMessageItem::Output(message)) => message
            .content
            .into_iter()
            .map(|part| match part {
                openai::ResponseMessageOutputContentPart::OutputText { text, .. } => text,
                openai::ResponseMessageOutputContentPart::Refusal { refusal, .. } => refusal,
            })
            .collect::<Vec<_>>()
            .join(""),
        openai::ResponseItem::Typed(_) | openai::ResponseItem::Unknown(_) => String::new(),
    }
}

fn openai_easy_content_text(content: openai::ResponseEasyInputContent) -> String {
    match content {
        openai::ResponseEasyInputContent::Text(text) => text,
        openai::ResponseEasyInputContent::Parts(parts) => response_input_parts_text(parts),
    }
}

fn response_input_parts_text(parts: Vec<openai::ResponseInputContentPart>) -> String {
    parts
        .into_iter()
        .filter_map(|part| match part {
            openai::ResponseInputContentPart::InputText { text, .. } => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub(in crate::transform::count_tokens) fn text_to_openai_input(
    text: String,
) -> Option<openai::ResponseInput> {
    if text.is_empty() {
        None
    } else {
        Some(openai::ResponseInput::Text(text))
    }
}

pub(in crate::transform::count_tokens) fn claude_messages_to_text(
    messages: Vec<claude::MessageParam>,
) -> String {
    messages
        .into_iter()
        .map(claude_message_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn claude_message_text(message: claude::MessageParam) -> String {
    match message.content {
        claude::StringOrArray::String(text) => text,
        claude::StringOrArray::Array(blocks) => blocks
            .into_iter()
            .filter_map(|block| match block {
                claude::ContentBlockParam::Text(text) => Some(text.text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
    }
}

pub(in crate::transform::count_tokens) fn text_to_claude_messages(
    text: String,
) -> Vec<claude::MessageParam> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![claude::MessageParam {
        role: claude::MessageRole::Known(claude::MessageRoleKnown::User),
        content: claude::StringOrArray::String(text),
        extra: Default::default(),
    }]
}

pub(in crate::transform::count_tokens) fn claude_system_to_text(
    system: Option<claude::SystemPrompt>,
) -> Option<String> {
    let system = system?;
    match system {
        claude::StringOrArray::String(text) => Some(text),
        claude::StringOrArray::Array(blocks) => {
            let text = blocks
                .into_iter()
                .map(|block| block.text)
                .collect::<Vec<_>>()
                .join("");
            if text.is_empty() { None } else { Some(text) }
        }
    }
}

pub(in crate::transform::count_tokens) fn text_to_claude_system(
    text: Option<String>,
) -> Option<claude::SystemPrompt> {
    text.filter(|value| !value.is_empty())
        .map(claude::StringOrArray::String)
}

pub(in crate::transform::count_tokens) fn gemini_contents_to_text(
    contents: Vec<gemini::Content>,
) -> String {
    contents
        .into_iter()
        .map(gemini_content_text)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(in crate::transform::count_tokens) fn gemini_content_text(content: gemini::Content) -> String {
    content
        .parts
        .into_iter()
        .filter_map(|part| match part.data {
            Some(gemini::PartData::Text { text }) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

pub(in crate::transform::count_tokens) fn text_to_gemini_contents(
    text: String,
) -> Vec<gemini::Content> {
    if text.is_empty() {
        return Vec::new();
    }
    vec![text_to_gemini_content(
        text,
        Some(gemini::ContentRole::Known(gemini::ContentRoleKnown::User)),
    )]
}

pub(in crate::transform::count_tokens) fn text_to_gemini_content(
    text: String,
    role: Option<gemini::ContentRole>,
) -> gemini::Content {
    gemini::Content {
        parts: vec![gemini::Part {
            thought: None,
            thought_signature: None,
            part_metadata: None,
            media_resolution: None,
            data: Some(gemini::PartData::Text { text }),
            metadata: None,
            extra: Default::default(),
        }],
        role,
        extra: Default::default(),
    }
}
