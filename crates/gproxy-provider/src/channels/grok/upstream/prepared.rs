use super::models::resolve_requested_model;
use super::upload::validate_data_uri_reference;
use super::*;
use crate::channels::grok::constants::{DEFAULT_IMAGE_MODEL, DEFAULT_VIDEO_MODEL};

impl GrokPreparedRequest {
    pub(super) fn from_transform_request(
        request: &TransformRequest,
    ) -> Result<Self, UpstreamError> {
        match request {
            TransformRequest::ModelListOpenAi(value) => {
                let _ = to_wreq_method(&value.method)?;
                Ok(Self::ModelList)
            }
            TransformRequest::ModelGetOpenAi(value) => {
                let _ = to_wreq_method(&value.method)?;
                Ok(Self::ModelGet {
                    target: value.path.model.clone(),
                })
            }
            TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
                let _ = to_wreq_method(&value.method)?;
                Self::from_chat_body(
                    serde_json::to_value(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    false,
                    extra_headers_from_transform_request(request),
                )
            }
            TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
                let _ = to_wreq_method(&value.method)?;
                Self::from_chat_body(
                    serde_json::to_value(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    true,
                    extra_headers_from_transform_request(request),
                )
            }
            TransformRequest::CreateImageOpenAi(value) => {
                let _ = to_wreq_method(&value.method)?;
                Self::from_image_body(
                    serde_json::to_value(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    false,
                    extra_headers_from_transform_request(request),
                )
            }
            TransformRequest::StreamCreateImageOpenAi(value) => {
                let _ = to_wreq_method(&value.method)?;
                Self::from_image_body(
                    serde_json::to_value(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    true,
                    extra_headers_from_transform_request(request),
                )
            }
            TransformRequest::CreateVideoOpenAi(value) => {
                let _ = to_wreq_method(&value.method)?;
                Self::from_video_create_body(
                    serde_json::to_value(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    extra_headers_from_transform_request(request),
                )
            }
            TransformRequest::VideoGetOpenAi(value) => {
                let _ = to_wreq_method(&value.method)?;
                Ok(Self::VideoGet {
                    video_id: value.path.video_id.clone(),
                })
            }
            TransformRequest::VideoContentGetOpenAi(value) => {
                let _ = to_wreq_method(&value.method)?;
                Ok(Self::VideoContentGet(GrokPreparedVideoContentRequest {
                    video_id: value.path.video_id.clone(),
                    variant: value
                        .query
                        .variant
                        .as_ref()
                        .map(|variant| {
                            serde_json::to_value(variant)
                                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
                                .and_then(|value| video_content_variant_from_value(&value))
                        })
                        .transpose()?
                        .unwrap_or(GrokPreparedVideoContentVariant::Video),
                }))
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }

    pub(super) fn from_payload(
        operation: OperationFamily,
        protocol: ProtocolKind,
        body: &[u8],
    ) -> Result<Self, UpstreamError> {
        if body.iter().all(|byte| byte.is_ascii_whitespace()) {
            return match (operation, protocol) {
                (OperationFamily::ModelList, ProtocolKind::OpenAi) => Ok(Self::ModelList),
                _ => Err(UpstreamError::UnsupportedRequest),
            };
        }

        let payload_value = serde_json::from_slice::<Value>(body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let extra_headers = extra_headers_from_payload_value(&payload_value);
        let body_value = payload_body_value(&payload_value);

        match (operation, protocol) {
            (OperationFamily::ModelList, ProtocolKind::OpenAi) => Ok(Self::ModelList),
            (OperationFamily::ModelGet, ProtocolKind::OpenAi) => {
                let Some(target) = json_pointer_string(&payload_value, "/path/model") else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in grok model_get payload".to_string(),
                    ));
                };
                Ok(Self::ModelGet { target })
            }
            (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion) => {
                Self::from_chat_body(body_value, false, extra_headers)
            }
            (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
                Self::from_chat_body(body_value, true, extra_headers)
            }
            (OperationFamily::CreateImage, ProtocolKind::OpenAi) => {
                Self::from_image_body(body_value, false, extra_headers)
            }
            (OperationFamily::StreamCreateImage, ProtocolKind::OpenAi) => {
                Self::from_image_body(body_value, true, extra_headers)
            }
            (OperationFamily::CreateVideo, ProtocolKind::OpenAi) => {
                Self::from_video_create_body(body_value, extra_headers)
            }
            (OperationFamily::VideoGet, ProtocolKind::OpenAi) => {
                let Some(video_id) = json_pointer_string(&payload_value, "/path/video_id") else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.video_id in grok video_get payload".to_string(),
                    ));
                };
                Ok(Self::VideoGet { video_id })
            }
            (OperationFamily::VideoContentGet, ProtocolKind::OpenAi) => {
                let Some(video_id) = json_pointer_string(&payload_value, "/path/video_id") else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.video_id in grok video content payload".to_string(),
                    ));
                };
                let variant = payload_value
                    .pointer("/query/variant")
                    .map(video_content_variant_from_value)
                    .transpose()?
                    .unwrap_or(GrokPreparedVideoContentVariant::Video);
                Ok(Self::VideoContentGet(GrokPreparedVideoContentRequest {
                    video_id,
                    variant,
                }))
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }

    fn from_chat_body(
        mut body: Value,
        stream: bool,
        extra_headers: Vec<(String, String)>,
    ) -> Result<Self, UpstreamError> {
        let Some(body_object) = body.as_object_mut() else {
            return Err(UpstreamError::SerializeRequest(
                "grok chat request body must be a JSON object".to_string(),
            ));
        };
        body_object.insert("stream".to_string(), Value::Bool(stream));

        let Some(request_model) = body_object
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
        else {
            return Err(UpstreamError::SerializeRequest(
                "missing model in grok chat request body".to_string(),
            ));
        };

        let tooling = build_tooling(body_object)?;
        let prompt = build_prompt_from_chat_body(body_object, &tooling)?;
        let cache_body = serde_json::to_vec(&body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;

        Ok(Self::Chat(GrokPreparedChatRequest {
            stream,
            resolved_model: resolve_requested_model(request_model.as_str()),
            request_model,
            extra_headers,
            prompt,
            tool_names: tooling.tool_names,
            cache_body,
        }))
    }

    fn from_image_body(
        body: Value,
        stream: bool,
        extra_headers: Vec<(String, String)>,
    ) -> Result<Self, UpstreamError> {
        let Some(body_object) = body.as_object() else {
            return Err(UpstreamError::SerializeRequest(
                "grok image request body must be a JSON object".to_string(),
            ));
        };
        let Some(prompt) = body_object
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
        else {
            return Err(UpstreamError::SerializeRequest(
                "missing prompt in grok image request body".to_string(),
            ));
        };

        let request_model = body_object
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_IMAGE_MODEL)
            .to_string();
        let n = body_object
            .get("n")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .clamp(1, 10) as u32;
        let request_size = body_object
            .get("size")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("1024x1024")
            .to_string();
        let response_format = match body_object
            .get("response_format")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("b64_json")
        {
            "url" => GrokImageResponseFormat::Url,
            _ => GrokImageResponseFormat::B64Json,
        };
        let output_format = body_object
            .get("output_format")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("png")
            .to_string();
        let quality = body_object
            .get("quality")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("auto")
            .to_string();
        let background = body_object
            .get("background")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("opaque")
            .to_string();

        Ok(Self::Image(GrokPreparedImageRequest {
            stream,
            request_model,
            extra_headers,
            prompt,
            n,
            aspect_ratio: image_size_to_aspect_ratio(request_size.as_str()),
            request_size,
            response_format,
            output_format,
            quality,
            background,
        }))
    }

    fn from_video_create_body(
        body: Value,
        extra_headers: Vec<(String, String)>,
    ) -> Result<Self, UpstreamError> {
        let Some(body_object) = body.as_object() else {
            return Err(UpstreamError::SerializeRequest(
                "grok video request body must be a JSON object".to_string(),
            ));
        };
        let Some(prompt) = body_object
            .get("prompt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
        else {
            return Err(UpstreamError::SerializeRequest(
                "missing prompt in grok video request body".to_string(),
            ));
        };

        let request_model = body_object
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_VIDEO_MODEL)
            .to_string();
        let size = body_object
            .get("size")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("1792x1024")
            .to_string();
        let seconds = match body_object.get("seconds") {
            Some(Value::String(value)) => value.trim().to_string(),
            Some(Value::Number(value)) => value.to_string(),
            _ => "6".to_string(),
        };
        let video_length = seconds
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or(6);

        Ok(Self::VideoCreate(GrokPreparedVideoCreateRequest {
            request_model,
            extra_headers,
            prompt,
            reference_url: parse_video_reference_url(body_object)?,
            aspect_ratio: video_size_to_aspect_ratio(size.as_str()),
            size,
            seconds,
            video_length,
            resolution_name: "480p".to_string(),
        }))
    }
}

fn json_pointer_string(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
}

fn image_size_to_aspect_ratio(size: &str) -> String {
    match size.trim() {
        "1024x1792" => "2:3".to_string(),
        "1792x1024" | "1536x1024" => "3:2".to_string(),
        _ => "1:1".to_string(),
    }
}

fn video_size_to_aspect_ratio(size: &str) -> String {
    match size.trim() {
        "720x1280" | "1024x1792" => "9:16".to_string(),
        "1280x720" | "1792x1024" => "16:9".to_string(),
        _ => "16:9".to_string(),
    }
}

fn parse_video_reference_url(body: &Map<String, Value>) -> Result<Option<String>, UpstreamError> {
    if let Some(image_reference) = body.get("image_reference").and_then(Value::as_object) {
        if image_reference
            .get("file_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
        {
            return Err(UpstreamError::SerializeRequest(
                "grok-web video does not support image_reference.file_id because no provider-side file lookup bridge is available".to_string(),
            ));
        }
        if let Some(url) = image_reference.get("image_url").and_then(Value::as_str) {
            return normalize_video_reference_url(url, "image_reference.image_url");
        }
    }

    if let Some(reference) = body.get("input_reference").and_then(Value::as_str) {
        return normalize_video_reference_url(reference, "input_reference");
    }

    Ok(None)
}

fn normalize_video_reference_url(raw: &str, field: &str) -> Result<Option<String>, UpstreamError> {
    let value = raw.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        return Ok(Some(value.to_string()));
    }
    if value.starts_with("data:") {
        validate_data_uri_reference(value).map_err(UpstreamError::SerializeRequest)?;
        return Ok(Some(value.to_string()));
    }
    Err(UpstreamError::SerializeRequest(format!(
        "{field} must be an http(s) URL or data URI"
    )))
}

fn video_content_variant_from_value(
    value: &Value,
) -> Result<GrokPreparedVideoContentVariant, UpstreamError> {
    let Some(value) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(GrokPreparedVideoContentVariant::Video);
    };
    match value {
        "video" => Ok(GrokPreparedVideoContentVariant::Video),
        "thumbnail" => Ok(GrokPreparedVideoContentVariant::Thumbnail),
        "spritesheet" => Ok(GrokPreparedVideoContentVariant::Spritesheet),
        _ => Err(UpstreamError::SerializeRequest(format!(
            "unsupported grok video content variant: {value}"
        ))),
    }
}

fn build_prompt_from_chat_body(
    body: &Map<String, Value>,
    tooling: &GrokTooling,
) -> Result<String, UpstreamError> {
    let mut sections = Vec::new();
    if let Some(prompt) = tooling
        .prompt_prefix
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        sections.push(prompt.to_string());
    }
    if let Some(instruction) = response_format_instruction(body.get("response_format"))? {
        sections.push(instruction);
    }
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        let rendered = messages_to_prompt(messages.as_slice());
        if !rendered.is_empty() {
            sections.push(rendered);
        }
    }
    Ok(sections.join("\n\n"))
}

fn response_format_instruction(
    response_format: Option<&Value>,
) -> Result<Option<String>, UpstreamError> {
    let Some(response_format) = response_format.and_then(Value::as_object) else {
        return Ok(None);
    };
    match response_format.get("type").and_then(Value::as_str) {
        None | Some("text") => Ok(None),
        Some("json_object") => Ok(Some(
            "Return valid JSON only. Do not wrap the result in markdown code fences.".to_string(),
        )),
        Some("json_schema") => {
            let schema = response_format
                .get("json_schema")
                .and_then(Value::as_object)
                .and_then(|item| item.get("schema"))
                .cloned()
                .unwrap_or(Value::Null);
            let schema_json = serde_json::to_string(&schema)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(Some(format!(
                "Return valid JSON only. Match this schema exactly: {schema_json}"
            )))
        }
        Some(_) => Ok(None),
    }
}

fn messages_to_prompt(messages: &[Value]) -> String {
    let mut extracted = Vec::new();
    for message in messages {
        let Some(object) = message.as_object() else {
            continue;
        };
        let Some(role) = object.get("role").and_then(Value::as_str) else {
            continue;
        };
        match role {
            "developer" => {
                let text = text_from_text_content(object.get("content"));
                if !text.is_empty() {
                    extracted.push(("developer".to_string(), text));
                }
            }
            "system" => {
                let text = text_from_text_content(object.get("content"));
                if !text.is_empty() {
                    extracted.push(("system".to_string(), text));
                }
            }
            "user" => {
                let text = text_from_user_content(object.get("content"));
                if !text.is_empty() {
                    extracted.push(("user".to_string(), text));
                }
            }
            "assistant" => {
                let text = text_from_assistant_message(object);
                if !text.is_empty() {
                    extracted.push(("assistant".to_string(), text));
                }
            }
            "tool" => {
                let text = text_from_text_content(object.get("content"));
                if !text.is_empty() {
                    let label = object
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(|value| format!("tool#{value}"))
                        .unwrap_or_else(|| "tool".to_string());
                    extracted.push((label, text));
                }
            }
            "function" => {
                let text = object
                    .get("content")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .unwrap_or_default();
                if !text.is_empty() {
                    let label = object
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(|value| format!("function[{value}]"))
                        .unwrap_or_else(|| "function".to_string());
                    extracted.push((label, text));
                }
            }
            _ => {}
        }
    }

    let last_user_index = extracted
        .iter()
        .enumerate()
        .rev()
        .find(|(_, (role, _))| role == "user")
        .map(|(index, _)| index);

    let mut out = Vec::new();
    for (index, (role, text)) in extracted.into_iter().enumerate() {
        if Some(index) == last_user_index {
            out.push(text);
        } else {
            out.push(format!("{role}: {text}"));
        }
    }
    out.join("\n\n")
}

fn text_from_text_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn text_from_user_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(Value::Array(parts)) => {
            let mut out = Vec::new();
            for part in parts {
                let Some(object) = part.as_object() else {
                    continue;
                };
                match object.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = object
                            .get("text")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                        {
                            out.push(text.to_string());
                        }
                    }
                    Some("image_url") => {
                        if let Some(url) = object
                            .get("image_url")
                            .and_then(Value::as_object)
                            .and_then(|value| value.get("url"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                        {
                            out.push(format!("[image] {url}"));
                        }
                    }
                    Some("input_audio") => {
                        if object.get("input_audio").is_some() {
                            out.push("[audio input omitted]".to_string());
                        }
                    }
                    Some("file") => {
                        if let Some(file) = object.get("file").and_then(Value::as_object) {
                            if let Some(url) = file
                                .get("file_url")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                            {
                                out.push(format!("[file] {url}"));
                            } else if let Some(name) = file
                                .get("filename")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                            {
                                out.push(format!("[file] {name}"));
                            } else if file.get("file_data").is_some()
                                || file.get("file_id").is_some()
                            {
                                out.push("[file input omitted]".to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
            out.join("\n")
        }
        _ => String::new(),
    }
}

fn text_from_assistant_message(message: &Map<String, Value>) -> String {
    let mut parts = Vec::new();

    match message.get("content") {
        Some(Value::String(value)) => {
            let text = value.trim();
            if !text.is_empty() {
                parts.push(text.to_string());
            }
        }
        Some(Value::Array(items)) => {
            for item in items {
                let Some(object) = item.as_object() else {
                    continue;
                };
                match object.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = object
                            .get("text")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                        {
                            parts.push(text.to_string());
                        }
                    }
                    Some("refusal") => {
                        if let Some(text) = object
                            .get("refusal")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                        {
                            parts.push(format!("[refusal] {text}"));
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }

    if let Some(reasoning) = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("[reasoning] {reasoning}"));
    }
    if let Some(refusal) = message
        .get("refusal")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("[refusal] {refusal}"));
    }
    if let Some(function_call) = message.get("function_call").and_then(Value::as_object) {
        if let Some(name) = function_call
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let arguments = function_call
                .get("arguments")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("{}");
            parts.push(format!("[tool_call] {name} {arguments}"));
        }
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let Some(object) = tool_call.as_object() else {
                continue;
            };
            if object.get("type").and_then(Value::as_str) != Some("function") {
                continue;
            }
            let Some(function) = object.get("function").and_then(Value::as_object) else {
                continue;
            };
            let Some(name) = function
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("{}");
            parts.push(format!("[tool_call] {name} {arguments}"));
        }
    }

    parts.join("\n")
}

fn build_tooling(body: &Map<String, Value>) -> Result<GrokTooling, UpstreamError> {
    let mut tools = Vec::new();
    if let Some(items) = body.get("tools").and_then(Value::as_array) {
        for tool in items {
            let Some(tool_object) = tool.as_object() else {
                continue;
            };
            if tool_object.get("type").and_then(Value::as_str) != Some("function") {
                continue;
            }
            let Some(function) = tool_object.get("function").and_then(Value::as_object) else {
                continue;
            };
            let Some(name) = function
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            tools.push(GrokFunctionTool {
                name: name.to_string(),
                description: function
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
                parameters: function.get("parameters").cloned(),
            });
        }
    } else if let Some(items) = body.get("functions").and_then(Value::as_array) {
        for function in items {
            let Some(function_object) = function.as_object() else {
                continue;
            };
            let Some(name) = function_object
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            tools.push(GrokFunctionTool {
                name: name.to_string(),
                description: function_object
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
                parameters: function_object.get("parameters").cloned(),
            });
        }
    }

    let tool_choice = tool_choice_from_chat_body(body);
    if tools.is_empty() || tool_choice == GrokToolChoiceMode::None {
        return Ok(GrokTooling::default());
    }

    let prompt_prefix = Some(build_tool_prompt(
        tools.as_slice(),
        tool_choice,
        body.get("parallel_tool_calls")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    )?);
    let tool_names = tools.iter().map(|item| item.name.clone()).collect();
    Ok(GrokTooling {
        prompt_prefix,
        tool_names,
    })
}

fn tool_choice_from_chat_body(body: &Map<String, Value>) -> GrokToolChoiceMode {
    if let Some(choice) = body.get("tool_choice") {
        return match choice {
            Value::String(mode) => match mode.trim() {
                "none" => GrokToolChoiceMode::None,
                "required" => GrokToolChoiceMode::Required,
                _ => GrokToolChoiceMode::Auto,
            },
            Value::Object(object) => match object.get("type").and_then(Value::as_str) {
                Some("function") | Some("custom") => GrokToolChoiceMode::Required,
                Some("allowed_tools") => match object
                    .get("allowed_tools")
                    .and_then(Value::as_object)
                    .and_then(|value| value.get("mode"))
                    .and_then(Value::as_str)
                {
                    Some("auto") => GrokToolChoiceMode::Auto,
                    Some("required") => GrokToolChoiceMode::Required,
                    _ => GrokToolChoiceMode::Required,
                },
                _ => GrokToolChoiceMode::Auto,
            },
            _ => GrokToolChoiceMode::Auto,
        };
    }
    if let Some(choice) = body.get("function_call") {
        return match choice {
            Value::String(mode) => match mode.trim() {
                "none" => GrokToolChoiceMode::None,
                "auto" => GrokToolChoiceMode::Auto,
                _ => GrokToolChoiceMode::Required,
            },
            Value::Object(_) => GrokToolChoiceMode::Required,
            _ => GrokToolChoiceMode::Auto,
        };
    }
    GrokToolChoiceMode::Auto
}

fn build_tool_prompt(
    tools: &[GrokFunctionTool],
    tool_choice: GrokToolChoiceMode,
    parallel_tool_calls: bool,
) -> Result<String, UpstreamError> {
    let mut lines = vec![
        "# Available Tools".to_string(),
        String::new(),
        "You can call tools by emitting <tool_call> JSON blocks.".to_string(),
        String::new(),
        "Format:".to_string(),
        "<tool_call>".to_string(),
        r#"{"name":"function_name","arguments":{"param":"value"}}"#.to_string(),
        "</tool_call>".to_string(),
        String::new(),
    ];
    if parallel_tool_calls {
        lines.push(
            "You may emit multiple <tool_call> blocks in a single response when needed."
                .to_string(),
        );
        lines.push(String::new());
    }
    lines.push("## Tool Definitions".to_string());
    lines.push(String::new());
    for tool in tools {
        lines.push(format!("### {}", tool.name));
        if let Some(description) = tool
            .description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(description.trim().to_string());
        }
        if let Some(parameters) = tool.parameters.as_ref() {
            lines.push(format!(
                "Parameters: {}",
                serde_json::to_string(parameters)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?
            ));
        }
        lines.push(String::new());
    }
    match tool_choice {
        GrokToolChoiceMode::None => {}
        GrokToolChoiceMode::Required => lines.push(
            "IMPORTANT: You must call at least one tool and not answer with plain text only."
                .to_string(),
        ),
        GrokToolChoiceMode::Auto => lines.push(
            "Call a tool only when it is needed. Otherwise answer with plain text.".to_string(),
        ),
    }
    Ok(lines.join("\n"))
}
