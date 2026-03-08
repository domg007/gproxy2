use super::stream::{
    encode_openai_chat_event, next_line, random_chat_completion_id, random_hex, unix_timestamp_secs,
};
use super::*;

pub(super) fn build_stream_http_response(
    response: wreq::Response,
    model: String,
    tool_names: Vec<String>,
) -> Result<wreq::Response, UpstreamError> {
    let mut parser = GrokLineStreamParser::new(model, tool_names);
    let stream = try_stream! {
        let mut line_buffer = Vec::new();
        let mut upstream = response.bytes_stream();
        while let Some(item) = upstream.next().await {
            let chunk = item.map_err(|err| std::io::Error::other(err.to_string()))?;
            line_buffer.extend_from_slice(chunk.as_ref());
            while let Some(line) = next_line(&mut line_buffer) {
                let events = parser
                    .on_line(line.as_slice())
                    .map_err(|err| std::io::Error::other(err.to_string()))?;
                for event in events {
                    yield encode_openai_chat_event(event)
                        .map_err(|err| std::io::Error::other(err.to_string()))?;
                }
            }
        }
        if !line_buffer.is_empty() {
            let events = parser
                .on_line(line_buffer.as_slice())
                .map_err(|err| std::io::Error::other(err.to_string()))?;
            for event in events {
                yield encode_openai_chat_event(event)
                    .map_err(|err| std::io::Error::other(err.to_string()))?;
            }
        }
        let tail_events = parser.finish();
        for event in tail_events {
            yield encode_openai_chat_event(event)
                .map_err(|err| std::io::Error::other(err.to_string()))?;
        }
    };
    build_http_stream_response(stream)
}

pub(super) async fn build_nonstream_http_response(
    response: wreq::Response,
    model: String,
    tool_names: Vec<String>,
) -> Result<wreq::Response, UpstreamError> {
    let events = collect_chat_stream_events(response, model, tool_names).await?;
    let completion = build_chat_completion_json(events.as_slice());
    build_json_http_response(StatusCode::OK, &completion)
}

async fn collect_chat_stream_events(
    response: wreq::Response,
    model: String,
    tool_names: Vec<String>,
) -> Result<Vec<OpenAiChatSseEvent>, UpstreamError> {
    let mut parser = GrokLineStreamParser::new(model, tool_names);
    let mut events = Vec::new();
    let mut line_buffer = Vec::new();
    let mut upstream = response.bytes_stream();
    while let Some(item) = upstream.next().await {
        let chunk = item.map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
        line_buffer.extend_from_slice(chunk.as_ref());
        while let Some(line) = next_line(&mut line_buffer) {
            events.extend(parser.on_line(line.as_slice())?);
        }
    }
    if !line_buffer.is_empty() {
        events.extend(parser.on_line(line_buffer.as_slice())?);
    }
    events.extend(parser.finish());
    Ok(events)
}

fn build_chat_completion_json(events: &[OpenAiChatSseEvent]) -> Value {
    let mut accumulator = ChatCompletionAccumulator::default();

    for event in events {
        let OpenAiChatSseEvent::Chunk(chunk) = event else {
            continue;
        };
        let Some(object) = chunk.as_object() else {
            continue;
        };
        if let Some(id) = object.get("id").and_then(Value::as_str) {
            accumulator.id = Some(id.to_string());
        }
        if let Some(created) = object.get("created").and_then(Value::as_u64) {
            accumulator.created = Some(created);
        }
        if let Some(model) = object.get("model").and_then(Value::as_str) {
            accumulator.model = Some(model.to_string());
        }
        if let Some(fingerprint) = object.get("system_fingerprint").and_then(Value::as_str) {
            accumulator.system_fingerprint = Some(fingerprint.to_string());
        }

        let Some(choice) = object
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(Value::as_object)
        else {
            continue;
        };
        if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
            accumulator.finish_reason = Some(finish_reason.to_string());
        }
        let Some(delta) = choice.get("delta").and_then(Value::as_object) else {
            continue;
        };
        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            accumulator.content.push_str(content);
        }
        if let Some(reasoning) = delta.get("reasoning_content").and_then(Value::as_str) {
            accumulator.reasoning_content.push_str(reasoning);
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for tool_call in tool_calls {
                let Some(object) = tool_call.as_object() else {
                    continue;
                };
                let index = object
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32)
                    .unwrap_or(accumulator.tool_calls.len() as u32);
                let entry = accumulator.tool_calls.entry(index).or_default();
                if let Some(id) = object.get("id").and_then(Value::as_str) {
                    entry.id = Some(id.to_string());
                }
                if let Some(type_) = object.get("type").and_then(Value::as_str) {
                    entry.type_ = Some(type_.to_string());
                }
                if let Some(function) = object.get("function").and_then(Value::as_object) {
                    if let Some(name) = function.get("name").and_then(Value::as_str) {
                        entry.name = Some(name.to_string());
                    }
                    if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                        entry.arguments.push_str(arguments);
                    }
                }
            }
        }
    }

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    if !accumulator.content.is_empty() {
        message.insert(
            "content".to_string(),
            Value::String(accumulator.content.clone()),
        );
    } else if accumulator.reasoning_content.is_empty() && accumulator.tool_calls.is_empty() {
        message.insert("content".to_string(), Value::String(String::new()));
    }
    if !accumulator.reasoning_content.is_empty() {
        message.insert(
            "reasoning_content".to_string(),
            Value::String(accumulator.reasoning_content.clone()),
        );
    }
    if !accumulator.tool_calls.is_empty() {
        let tool_calls = accumulator
            .tool_calls
            .into_values()
            .filter_map(|tool_call| {
                let name = tool_call.name?;
                Some(json!({
                    "id": tool_call
                        .id
                        .unwrap_or_else(|| format!("call_{}", random_hex(24))),
                    "function": {
                        "name": name,
                        "arguments": if tool_call.arguments.is_empty() {
                            "{}".to_string()
                        } else {
                            tool_call.arguments
                        },
                    },
                    "type": tool_call.type_.unwrap_or_else(|| "function".to_string()),
                }))
            })
            .collect::<Vec<_>>();
        if !tool_calls.is_empty() {
            message.insert("tool_calls".to_string(), Value::Array(tool_calls));
        }
    }

    let finish_reason = accumulator.finish_reason.unwrap_or_else(|| {
        if message.contains_key("tool_calls") {
            "tool_calls".to_string()
        } else {
            "stop".to_string()
        }
    });

    let mut choice = Map::new();
    choice.insert("finish_reason".to_string(), Value::String(finish_reason));
    choice.insert("index".to_string(), Value::from(0_u64));
    choice.insert("message".to_string(), Value::Object(message));

    let mut response = Map::new();
    response.insert(
        "id".to_string(),
        Value::String(accumulator.id.unwrap_or_else(random_chat_completion_id)),
    );
    response.insert(
        "choices".to_string(),
        Value::Array(vec![Value::Object(choice)]),
    );
    response.insert(
        "created".to_string(),
        Value::from(accumulator.created.unwrap_or_else(unix_timestamp_secs)),
    );
    response.insert(
        "model".to_string(),
        Value::String(
            accumulator
                .model
                .unwrap_or_else(|| "grok-unknown".to_string()),
        ),
    );
    response.insert(
        "object".to_string(),
        Value::String("chat.completion".to_string()),
    );
    if let Some(fingerprint) = accumulator.system_fingerprint {
        response.insert("system_fingerprint".to_string(), Value::String(fingerprint));
    }

    Value::Object(response)
}

pub(super) async fn build_openai_error_http_response(
    response: wreq::Response,
) -> Result<wreq::Response, UpstreamError> {
    let status = response.status();
    let text = response
        .text()
        .await
        .unwrap_or_else(|_| "upstream request failed".to_string());
    let error_type = if status == StatusCode::UNAUTHORIZED {
        "authentication_error"
    } else if status == StatusCode::TOO_MANY_REQUESTS {
        "rate_limit_error"
    } else {
        "invalid_request_error"
    };
    build_json_http_response(
        status,
        &openai_error_body(text.trim().to_string(), error_type, None, None),
    )
}

pub(super) fn openai_error_body(
    message: String,
    type_: &str,
    param: Option<&str>,
    code: Option<&str>,
) -> Value {
    json!({
        "error": {
            "message": message,
            "type": type_,
            "param": param,
            "code": code,
        }
    })
}

pub(super) fn build_json_http_response<T: serde::Serialize>(
    status: StatusCode,
    body: &T,
) -> Result<wreq::Response, UpstreamError> {
    let bytes =
        serde_json::to_vec(body).map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    Ok(wreq::Response::from(
        HttpResponse::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(bytes)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
    ))
}

pub(super) fn build_openai_error_json_response(
    status: StatusCode,
    message: impl Into<String>,
    type_: &str,
    param: Option<&str>,
    code: Option<&str>,
) -> Result<wreq::Response, UpstreamError> {
    build_json_http_response(
        status,
        &openai_error_body(message.into(), type_, param, code),
    )
}

pub(super) fn build_http_stream_response<S>(stream: S) -> Result<wreq::Response, UpstreamError>
where
    S: futures_util::TryStream<Ok = Bytes, Error = std::io::Error> + Send + 'static,
{
    Ok(wreq::Response::from(
        HttpResponse::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream; charset=utf-8")
            .header("cache-control", "no-cache")
            .header("x-accel-buffering", "no")
            .body(WreqBody::wrap_stream(stream))
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
    ))
}

pub(super) fn local_http_response(response: wreq::Response) -> UpstreamResponse {
    UpstreamResponse {
        credential_id: None,
        attempts: 0,
        response: Some(response),
        local_response: None,
        credential_update: None,
        request_meta: None,
    }
}
