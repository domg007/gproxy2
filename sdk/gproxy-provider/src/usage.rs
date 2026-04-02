use crate::engine::Usage;

/// Extract usage from a non-streaming response body based on the upstream protocol.
pub fn extract_usage(protocol: &str, body: &[u8]) -> Option<Usage> {
    match protocol {
        "openai_response" | "openai_chat_completions" | "openai" => extract_openai_usage(body),
        "claude" => extract_claude_usage(body),
        "gemini" => extract_gemini_usage(body),
        _ => None,
    }
}

/// Extract usage from a single streaming event/chunk.
/// Call this on each chunk; the last non-None result is the final usage.
pub fn extract_stream_usage(protocol: &str, chunk: &[u8]) -> Option<Usage> {
    match protocol {
        "openai_chat_completions" => extract_openai_chunk_usage(chunk),
        "openai_response" | "openai" => extract_openai_response_event_usage(chunk),
        "claude" => extract_claude_event_usage(chunk),
        "gemini" => extract_gemini_usage(chunk),
        _ => None,
    }
}

// === Non-streaming extractors ===

fn extract_openai_usage(body: &[u8]) -> Option<Usage> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let usage = v.get("usage")?;
    Some(Usage {
        input_tokens: usage.get("prompt_tokens").and_then(|v| v.as_i64()),
        output_tokens: usage.get("completion_tokens").and_then(|v| v.as_i64()),
        cache_read_input_tokens: usage
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(|v| v.as_i64()),
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    })
}

fn extract_claude_usage(body: &[u8]) -> Option<Usage> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let usage = v.get("usage")?;
    Some(Usage {
        input_tokens: usage.get("input_tokens").and_then(|v| v.as_i64()),
        output_tokens: usage.get("output_tokens").and_then(|v| v.as_i64()),
        cache_read_input_tokens: usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_i64()),
        cache_creation_input_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_i64()),
        cache_creation_input_tokens_5min: usage
            .get("cache_creation")
            .and_then(|c| c.get("ephemeral_5m_input_tokens"))
            .and_then(|v| v.as_i64()),
        cache_creation_input_tokens_1h: usage
            .get("cache_creation")
            .and_then(|c| c.get("ephemeral_1h_input_tokens"))
            .and_then(|v| v.as_i64()),
    })
}

fn extract_gemini_usage(body: &[u8]) -> Option<Usage> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let usage = v.get("usageMetadata")?;
    Some(Usage {
        input_tokens: usage.get("promptTokenCount").and_then(|v| v.as_i64()),
        output_tokens: usage.get("candidatesTokenCount").and_then(|v| v.as_i64()),
        cache_read_input_tokens: usage
            .get("cachedContentTokenCount")
            .and_then(|v| v.as_i64()),
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    })
}

// === Stream event extractors ===

/// OpenAI ChatCompletions: usage in last chunk's `usage` field
fn extract_openai_chunk_usage(chunk: &[u8]) -> Option<Usage> {
    let v: serde_json::Value = serde_json::from_slice(chunk).ok()?;
    let usage = v.get("usage")?;
    if usage.is_null() {
        return None;
    }
    Some(Usage {
        input_tokens: usage.get("prompt_tokens").and_then(|v| v.as_i64()),
        output_tokens: usage.get("completion_tokens").and_then(|v| v.as_i64()),
        cache_read_input_tokens: usage
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(|v| v.as_i64()),
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    })
}

/// OpenAI Responses: usage in `response.completed` event's `response.usage`
fn extract_openai_response_event_usage(chunk: &[u8]) -> Option<Usage> {
    let v: serde_json::Value = serde_json::from_slice(chunk).ok()?;
    if v.get("type")?.as_str()? != "response.completed" {
        return None;
    }
    let usage = v.get("response")?.get("usage")?;
    Some(Usage {
        input_tokens: usage.get("input_tokens").and_then(|v| v.as_i64()),
        output_tokens: usage.get("output_tokens").and_then(|v| v.as_i64()),
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
    })
}

/// Claude: usage in `message_delta` event's `usage` field
fn extract_claude_event_usage(chunk: &[u8]) -> Option<Usage> {
    let v: serde_json::Value = serde_json::from_slice(chunk).ok()?;
    if v.get("type")?.as_str()? != "message_delta" {
        return None;
    }
    let usage = v.get("usage")?;
    Some(Usage {
        input_tokens: usage.get("input_tokens").and_then(|v| v.as_i64()),
        output_tokens: usage.get("output_tokens").and_then(|v| v.as_i64()),
        cache_read_input_tokens: usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_i64()),
        cache_creation_input_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_i64()),
        cache_creation_input_tokens_5min: usage
            .get("cache_creation")
            .and_then(|c| c.get("ephemeral_5m_input_tokens"))
            .and_then(|v| v.as_i64()),
        cache_creation_input_tokens_1h: usage
            .get("cache_creation")
            .and_then(|c| c.get("ephemeral_1h_input_tokens"))
            .and_then(|v| v.as_i64()),
    })
}
