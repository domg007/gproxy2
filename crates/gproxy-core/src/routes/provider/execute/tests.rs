use gproxy_middleware::TransformResponse;
use gproxy_middleware::{OperationFamily, ProtocolKind};
use gproxy_protocol::openai::model_list::response::OpenAiModelListResponse;
use serde_json::json;

use super::{
    encode_transform_stream_error_chunk, serialize_local_response_body,
    wrap_payload_for_typed_decode,
};

#[test]
fn local_response_body_is_unwrapped_from_enum_shell_and_http_wrapper() {
    let response: OpenAiModelListResponse = serde_json::from_value(json!({
        "stats_code": 200,
        "headers": {},
        "body": {
            "object": "list",
            "data": []
        }
    }))
    .expect("valid openai model list response");

    let bytes = serialize_local_response_body(&TransformResponse::ModelListOpenAi(response))
        .expect("serialize local response");
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("decode serialized local response");

    assert!(value.get("ModelListOpenAi").is_none());
    assert!(value.get("stats_code").is_none());
    assert_eq!(value.get("object").and_then(|v| v.as_str()), Some("list"));
    assert!(value.get("data").is_some());
}

#[test]
fn stream_transform_error_chunk_is_ndjson_for_gemini_ndjson() {
    let chunk = encode_transform_stream_error_chunk(ProtocolKind::GeminiNDJson, "boom".to_string());
    let text = String::from_utf8(chunk.to_vec()).expect("utf8");
    assert!(text.ends_with('\n'));

    let value: serde_json::Value = serde_json::from_str(text.trim()).expect("json");
    assert_eq!(
        value
            .get("error")
            .and_then(|v| v.get("message"))
            .and_then(|v| v.as_str()),
        Some("boom")
    );
    assert_eq!(
        value
            .get("error")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str()),
        Some("transform_serialization_error")
    );
}

#[test]
fn stream_transform_error_chunk_is_sse_for_non_ndjson() {
    let chunk = encode_transform_stream_error_chunk(ProtocolKind::OpenAi, "boom".to_string());
    let text = String::from_utf8(chunk.to_vec()).expect("utf8");
    assert!(text.starts_with("event: error\n"));
    assert!(text.ends_with("\n\n"));

    let data_line = text
        .lines()
        .find(|line| line.starts_with("data: "))
        .expect("data line");
    let payload = data_line.trim_start_matches("data: ");
    let value: serde_json::Value = serde_json::from_str(payload).expect("json");
    assert_eq!(
        value
            .get("error")
            .and_then(|v| v.get("message"))
            .and_then(|v| v.as_str()),
        Some("boom")
    );
}

#[test]
fn wrap_openai_body_into_full_envelope_for_typed_decode() {
    let raw = serde_json::to_vec(&json!({
        "model": "gpt-5",
        "messages": [{"role": "user", "content": "ping"}],
        "stream": false
    }))
    .expect("serialize raw body");

    let wrapped = wrap_payload_for_typed_decode(
        OperationFamily::GenerateContent,
        ProtocolKind::OpenAiChatCompletion,
        raw,
    )
    .expect("wrap payload");
    let value: serde_json::Value = serde_json::from_slice(&wrapped).expect("decode wrapped");

    assert_eq!(value.get("method").and_then(|v| v.as_str()), Some("POST"));
    assert!(value.get("path").is_some());
    assert!(value.get("query").is_some());
    assert!(value.get("headers").is_some());
    assert_eq!(
        value
            .get("body")
            .and_then(|v| v.get("model"))
            .and_then(|v| v.as_str()),
        Some("gpt-5")
    );
}

#[test]
fn wrap_claude_partial_envelope_with_defaults() {
    let raw = serde_json::to_vec(&json!({
        "headers": {"anthropic-version": "2023-06-01"},
        "body": {"model": "claude-sonnet-4", "messages": [], "max_tokens": 16}
    }))
    .expect("serialize raw body");

    let wrapped =
        wrap_payload_for_typed_decode(OperationFamily::GenerateContent, ProtocolKind::Claude, raw)
            .expect("wrap payload");
    let value: serde_json::Value = serde_json::from_slice(&wrapped).expect("decode wrapped");

    assert_eq!(value.get("method").and_then(|v| v.as_str()), Some("POST"));
    assert!(value.get("path").is_some());
    assert!(value.get("query").is_some());
    assert_eq!(
        value
            .get("headers")
            .and_then(|v| v.get("anthropic-version"))
            .and_then(|v| v.as_str()),
        Some("2023-06-01")
    );
    assert_eq!(
        value
            .get("body")
            .and_then(|v| v.get("model"))
            .and_then(|v| v.as_str()),
        Some("claude-sonnet-4")
    );
}

#[test]
fn wrap_gemini_partial_envelope_with_defaults() {
    let raw = serde_json::to_vec(&json!({
        "path": {"model": "models/gemini-2.5-pro"},
        "query": {"alt": "sse"},
        "body": {"contents": []}
    }))
    .expect("serialize raw body");

    let wrapped = wrap_payload_for_typed_decode(
        OperationFamily::StreamGenerateContent,
        ProtocolKind::Gemini,
        raw,
    )
    .expect("wrap payload");
    let value: serde_json::Value = serde_json::from_slice(&wrapped).expect("decode wrapped");

    assert_eq!(value.get("method").and_then(|v| v.as_str()), Some("POST"));
    assert_eq!(
        value
            .get("path")
            .and_then(|v| v.get("model"))
            .and_then(|v| v.as_str()),
        Some("models/gemini-2.5-pro")
    );
    assert_eq!(
        value
            .get("query")
            .and_then(|v| v.get("alt"))
            .and_then(|v| v.as_str()),
        Some("sse")
    );
    assert!(value.get("headers").is_some());
    assert!(value.get("body").is_some());
}
