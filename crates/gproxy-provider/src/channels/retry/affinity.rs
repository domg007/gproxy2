use super::*;

pub fn cache_affinity_protocol_from_transform_request(
    request: &TransformRequest,
) -> Option<CacheAffinityProtocol> {
    match request {
        TransformRequest::GenerateContentOpenAiResponse(_)
        | TransformRequest::StreamGenerateContentOpenAiResponse(_) => {
            Some(CacheAffinityProtocol::OpenAiResponses)
        }
        TransformRequest::GenerateContentOpenAiChatCompletions(_)
        | TransformRequest::StreamGenerateContentOpenAiChatCompletions(_) => {
            Some(CacheAffinityProtocol::OpenAiChatCompletions)
        }
        TransformRequest::GenerateContentClaude(_)
        | TransformRequest::StreamGenerateContentClaude(_) => {
            Some(CacheAffinityProtocol::ClaudeMessages)
        }
        TransformRequest::GenerateContentGemini(_)
        | TransformRequest::StreamGenerateContentGeminiSse(_)
        | TransformRequest::StreamGenerateContentGeminiNdjson(_) => {
            Some(CacheAffinityProtocol::GeminiGenerateContent)
        }
        _ => None,
    }
}

pub fn cache_affinity_hint_from_transform_request(
    protocol: CacheAffinityProtocol,
    model: Option<&str>,
    body: Option<&[u8]>,
) -> Option<CacheAffinityHint> {
    let body_json = serde_json::from_slice::<Value>(body?).ok()?;
    match protocol {
        CacheAffinityProtocol::OpenAiResponses => {
            cache_affinity_hint_for_openai_responses(body_json)
        }
        CacheAffinityProtocol::OpenAiChatCompletions => {
            cache_affinity_hint_for_openai_chat(body_json)
        }
        CacheAffinityProtocol::ClaudeMessages => {
            cache_affinity_hint_for_claude_effective_body(body_json)
        }
        CacheAffinityProtocol::GeminiGenerateContent => {
            cache_affinity_hint_for_gemini(model.unwrap_or("unknown"), body_json)
        }
    }
}

pub fn cache_affinity_hint_from_codex_transform_request(
    request: &TransformRequest,
    model: Option<&str>,
    body: Option<&[u8]>,
) -> Option<CacheAffinityHint> {
    let protocol = cache_affinity_protocol_from_transform_request(request)?;
    if matches!(protocol, CacheAffinityProtocol::OpenAiResponses) {
        return cache_affinity_hint_from_codex_openai_response_body(model, body);
    }
    cache_affinity_hint_from_transform_request(protocol, model, body)
}

pub fn cache_affinity_hint_from_codex_openai_response_body(
    model: Option<&str>,
    body: Option<&[u8]>,
) -> Option<CacheAffinityHint> {
    cache_affinity_hint_for_codex_openai_responses(body).or_else(|| {
        cache_affinity_hint_from_transform_request(
            CacheAffinityProtocol::OpenAiResponses,
            model,
            body,
        )
    })
}

pub(super) fn cache_affinity_hint_for_codex_openai_responses(
    body: Option<&[u8]>,
) -> Option<CacheAffinityHint> {
    let body_json = serde_json::from_slice::<Value>(body?).ok()?;
    let session_marker = body_json
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            let conversation = body_json.get("conversation")?;
            match conversation {
                Value::String(value) => {
                    let value = value.trim();
                    (!value.is_empty()).then(|| value.to_string())
                }
                Value::Object(value) => value
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(ToString::to_string),
                _ => None,
            }
        })
        .or_else(|| {
            body_json
                .get("previous_response_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })?;

    let key = format!(
        "codex.responses.session:{}",
        hash_str_to_hex(session_marker.as_str())
    );
    let key_len = key.len();
    let candidate = CacheAffinityCandidate {
        key,
        ttl_ms: OPENAI_24H_CACHE_AFFINITY_TTL_MS,
        key_len,
    };
    Some(CacheAffinityHint {
        candidates: vec![candidate.clone()],
        bind: candidate,
    })
}

pub fn cache_affinity_hint_for_claude_effective_body(
    body_json: Value,
) -> Option<CacheAffinityHint> {
    cache_affinity_hint_for_claude(body_json, DEFAULT_CACHE_AFFINITY_TTL_MS)
}

pub fn credential_pick_mode(
    configured_pick_mode: CredentialPickMode,
    cache_affinity_hint: Option<&CacheAffinityHint>,
) -> CredentialPickMode {
    match configured_pick_mode {
        CredentialPickMode::RoundRobinWithCache => {
            if cache_affinity_hint.is_some() {
                CredentialPickMode::RoundRobinWithCache
            } else {
                CredentialPickMode::RoundRobinNoCache
            }
        }
        CredentialPickMode::RoundRobinNoCache => CredentialPickMode::RoundRobinNoCache,
        CredentialPickMode::StickyNoCache => CredentialPickMode::StickyNoCache,
    }
}

pub fn configured_pick_mode_uses_cache(configured_pick_mode: CredentialPickMode) -> bool {
    matches!(
        configured_pick_mode,
        CredentialPickMode::RoundRobinWithCache
    )
}

pub(super) fn cache_affinity_hint_for_openai_responses(
    body_json: Value,
) -> Option<CacheAffinityHint> {
    let ttl_ms = openai_prompt_cache_ttl_ms(body_json.get("prompt_cache_retention"));
    let retention = openai_retention_tag(body_json.get("prompt_cache_retention"));
    let model = body_json
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let prompt_cache_key_hash = body_json
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(hash_str_to_hex)
        .unwrap_or_else(|| "none".to_string());

    let blocks = openai_responses_cache_blocks(&body_json);
    non_claude_affinity_hint("openai.responses", model, ttl_ms, blocks, |prefix_hash| {
        format!("openai.responses:ret={retention}:k={prompt_cache_key_hash}:h={prefix_hash}")
    })
}

pub(super) fn cache_affinity_hint_for_openai_chat(body_json: Value) -> Option<CacheAffinityHint> {
    let ttl_ms = openai_prompt_cache_ttl_ms(body_json.get("prompt_cache_retention"));
    let retention = openai_retention_tag(body_json.get("prompt_cache_retention"));
    let model = body_json
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let prompt_cache_key_hash = body_json
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(hash_str_to_hex)
        .unwrap_or_else(|| "none".to_string());

    let blocks = openai_chat_cache_blocks(&body_json);
    non_claude_affinity_hint("openai.chat", model, ttl_ms, blocks, |prefix_hash| {
        format!("openai.chat:ret={retention}:k={prompt_cache_key_hash}:h={prefix_hash}")
    })
}

pub(super) fn cache_affinity_hint_for_claude(
    body_json: Value,
    default_ttl_ms: u64,
) -> Option<CacheAffinityHint> {
    let blocks = claude_cache_blocks(&body_json, default_ttl_ms);
    if blocks.is_empty() {
        return None;
    }

    let hashes = build_prefix_hashes(
        "claude.messages",
        &blocks
            .iter()
            .map(|b| b.hash_value.clone())
            .collect::<Vec<_>>(),
    )?;
    if hashes.is_empty() {
        return None;
    }

    let mut breakpoints = claude_breakpoints(&body_json, &blocks, default_ttl_ms);
    if breakpoints.is_empty() {
        return None;
    }

    breakpoints.sort_by(|left, right| {
        right
            .index
            .cmp(&left.index)
            .then_with(|| left.kind.cmp(right.kind))
    });

    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    for breakpoint in breakpoints {
        let start = breakpoint
            .index
            .saturating_sub(CLAUDE_BREAKPOINT_LOOKBACK.saturating_sub(1));
        for idx in (start..=breakpoint.index).rev() {
            let Some(prefix_hash) = hashes.get(idx) else {
                continue;
            };
            let ttl_tag = ttl_tag(breakpoint.ttl_ms);
            let key = format!(
                "claude.messages:ttl={ttl_tag}:bp={}:h={prefix_hash}",
                breakpoint.kind
            );
            if seen.insert(key.clone()) {
                let key_len = key.len();
                candidates.push(CacheAffinityCandidate {
                    key,
                    ttl_ms: breakpoint.ttl_ms,
                    key_len,
                });
            }
        }
    }

    let bind = candidates.first()?.clone();
    Some(CacheAffinityHint { candidates, bind })
}

pub(super) fn cache_affinity_hint_for_gemini(
    model: &str,
    body_json: Value,
) -> Option<CacheAffinityHint> {
    if let Some(cached_content) = body_json
        .get("cachedContent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let key = format!("gemini.cachedContent:{}", hash_str_to_hex(cached_content));
        let key_len = key.len();
        let candidate = CacheAffinityCandidate {
            key,
            ttl_ms: GEMINI_CACHED_CONTENT_TTL_MS,
            key_len,
        };
        return Some(CacheAffinityHint {
            candidates: vec![candidate.clone()],
            bind: candidate,
        });
    }

    let blocks = gemini_cache_blocks(&body_json);
    non_claude_affinity_hint(
        "gemini.generateContent",
        model,
        DEFAULT_CACHE_AFFINITY_TTL_MS,
        blocks,
        |prefix_hash| format!("gemini.generateContent:prefix:{prefix_hash}"),
    )
}

pub(super) fn non_claude_affinity_hint<F>(
    seed: &str,
    model: &str,
    ttl_ms: u64,
    blocks: Vec<Value>,
    key_builder: F,
) -> Option<CacheAffinityHint>
where
    F: Fn(&str) -> String,
{
    if blocks.is_empty() {
        return None;
    }

    let hash_seed = format!("{seed}:{model}");
    let prefix_hashes = build_prefix_hashes(hash_seed.as_str(), &blocks)?;
    let bind_hash = prefix_hashes.last()?;

    let mut candidates = Vec::new();
    for idx in non_claude_candidate_indices(prefix_hashes.len()) {
        let Some(prefix_hash) = prefix_hashes.get(idx) else {
            continue;
        };
        let key = key_builder(prefix_hash);
        let key_len = key.len();
        candidates.push(CacheAffinityCandidate {
            key,
            ttl_ms,
            key_len,
        });
    }

    if candidates.is_empty() {
        return None;
    }

    let bind_key = key_builder(bind_hash);
    let bind = CacheAffinityCandidate {
        key_len: bind_key.len(),
        key: bind_key,
        ttl_ms,
    };

    Some(CacheAffinityHint { candidates, bind })
}

pub(super) fn openai_chat_cache_blocks(body_json: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();

    if let Some(tools) = body_json.get("tools").and_then(Value::as_array) {
        for (idx, tool) in tools.iter().enumerate() {
            blocks.push(json!({
                "kind": "tools",
                "index": idx,
                "value": tool,
            }));
        }
    }

    if let Some(json_schema) = body_json
        .get("response_format")
        .and_then(|value| value.get("json_schema"))
    {
        blocks.push(json!({
            "kind": "response_format_json_schema",
            "value": json_schema,
        }));
    }

    if let Some(messages) = body_json.get("messages").and_then(Value::as_array) {
        for (message_index, message) in messages.iter().enumerate() {
            push_content_blocks(&mut blocks, "messages", message_index, message, "content");
        }
    }

    blocks
}

pub(super) fn openai_responses_cache_blocks(body_json: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();

    if let Some(tools) = body_json.get("tools").and_then(Value::as_array) {
        for (idx, tool) in tools.iter().enumerate() {
            blocks.push(json!({
                "kind": "tools",
                "index": idx,
                "value": tool,
            }));
        }
    }

    if let Some(prompt) = body_json.get("prompt").and_then(Value::as_object) {
        let mut prompt_value = serde_json::Map::new();
        if let Some(id) = prompt.get("id") {
            prompt_value.insert("id".to_string(), id.clone());
        }
        if let Some(version) = prompt.get("version") {
            prompt_value.insert("version".to_string(), version.clone());
        }
        if let Some(variables) = prompt.get("variables") {
            prompt_value.insert("variables".to_string(), variables.clone());
        }
        if !prompt_value.is_empty() {
            blocks.push(json!({
                "kind": "prompt",
                "value": Value::Object(prompt_value),
            }));
        }
    }

    if let Some(instructions) = body_json.get("instructions") {
        blocks.push(json!({
            "kind": "instructions",
            "value": instructions,
        }));
    }

    if let Some(input) = body_json.get("input") {
        match input {
            Value::Array(items) => {
                for (idx, item) in items.iter().enumerate() {
                    push_content_blocks(&mut blocks, "input", idx, item, "content");
                }
            }
            _ => {
                blocks.push(json!({
                    "kind": "input",
                    "index": 0,
                    "value": input,
                }));
            }
        }
    }

    blocks
}

pub(super) fn gemini_cache_blocks(body_json: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();

    if let Some(system_instruction) = body_json.get("systemInstruction") {
        blocks.push(json!({
            "kind": "system_instruction",
            "value": system_instruction,
        }));
    }

    if let Some(tools) = body_json.get("tools").and_then(Value::as_array) {
        for (idx, tool) in tools.iter().enumerate() {
            blocks.push(json!({
                "kind": "tools",
                "index": idx,
                "value": tool,
            }));
        }
    }

    if let Some(tool_config) = body_json.get("toolConfig") {
        blocks.push(json!({
            "kind": "tool_config",
            "value": tool_config,
        }));
    }

    if let Some(contents) = body_json.get("contents").and_then(Value::as_array) {
        for (content_index, content) in contents.iter().enumerate() {
            push_content_blocks(&mut blocks, "contents", content_index, content, "parts");
        }
    }

    blocks
}

pub(super) fn claude_cache_blocks(body_json: &Value, default_ttl_ms: u64) -> Vec<ClaudeCacheBlock> {
    let mut blocks = Vec::new();

    if let Some(tools) = body_json.get("tools").and_then(Value::as_array) {
        for (tool_index, tool) in tools.iter().enumerate() {
            let explicit_ttl_ms = tool
                .get("cache_control")
                .map(|value| claude_cache_control_ttl_ms_from_value(value, default_ttl_ms));
            blocks.push(ClaudeCacheBlock {
                hash_value: json!({
                    "section": "tools",
                    "index": tool_index,
                    "value": tool,
                }),
                explicit_ttl_ms,
                cacheable: claude_block_is_cacheable(tool),
            });
        }
    }

    if let Some(system) = body_json.get("system") {
        match system {
            Value::String(text) => {
                let raw = json!({ "type": "text", "text": text });
                blocks.push(ClaudeCacheBlock {
                    hash_value: json!({
                        "section": "system",
                        "index": 0,
                        "value": raw,
                    }),
                    explicit_ttl_ms: None,
                    cacheable: claude_block_is_cacheable(&raw),
                });
            }
            Value::Array(items) => {
                for (idx, item) in items.iter().enumerate() {
                    let explicit_ttl_ms = item
                        .get("cache_control")
                        .map(|value| claude_cache_control_ttl_ms_from_value(value, default_ttl_ms));
                    blocks.push(ClaudeCacheBlock {
                        hash_value: json!({
                            "section": "system",
                            "index": idx,
                            "value": item,
                        }),
                        explicit_ttl_ms,
                        cacheable: claude_block_is_cacheable(item),
                    });
                }
            }
            _ => {}
        }
    }

    if let Some(messages) = body_json.get("messages").and_then(Value::as_array) {
        for (message_index, message) in messages.iter().enumerate() {
            let role = message.get("role").cloned().unwrap_or(Value::Null);
            let content = message.get("content");
            match content {
                Some(Value::String(text)) => {
                    let raw = json!({ "type": "text", "text": text });
                    blocks.push(ClaudeCacheBlock {
                        hash_value: json!({
                            "section": "messages",
                            "message_index": message_index,
                            "role": role,
                            "content_index": 0,
                            "value": raw,
                        }),
                        explicit_ttl_ms: None,
                        cacheable: claude_block_is_cacheable(&raw),
                    });
                }
                Some(Value::Array(items)) => {
                    for (content_index, item) in items.iter().enumerate() {
                        let explicit_ttl_ms = item.get("cache_control").map(|value| {
                            claude_cache_control_ttl_ms_from_value(value, default_ttl_ms)
                        });
                        blocks.push(ClaudeCacheBlock {
                            hash_value: json!({
                                "section": "messages",
                                "message_index": message_index,
                                "role": role,
                                "content_index": content_index,
                                "value": item,
                            }),
                            explicit_ttl_ms,
                            cacheable: claude_block_is_cacheable(item),
                        });
                    }
                }
                Some(other) => {
                    blocks.push(ClaudeCacheBlock {
                        hash_value: json!({
                            "section": "messages",
                            "message_index": message_index,
                            "role": role,
                            "content_index": 0,
                            "value": other,
                        }),
                        explicit_ttl_ms: None,
                        cacheable: claude_block_is_cacheable(other),
                    });
                }
                None => {}
            }
        }
    }

    blocks
}

pub(super) fn claude_breakpoints(
    body_json: &Value,
    blocks: &[ClaudeCacheBlock],
    default_ttl_ms: u64,
) -> Vec<ClaudeBreakpoint> {
    let mut breakpoints = Vec::new();

    for (idx, block) in blocks.iter().enumerate() {
        if let Some(ttl_ms) = block.explicit_ttl_ms {
            breakpoints.push(ClaudeBreakpoint {
                index: idx,
                ttl_ms,
                kind: "explicit",
            });
        }
    }

    if let Some(cache_control) = body_json.get("cache_control") {
        let ttl_ms = claude_auto_cache_control_ttl_ms_from_value(cache_control, default_ttl_ms);
        if let Some(index) = blocks.iter().rposition(|block| block.cacheable) {
            breakpoints.push(ClaudeBreakpoint {
                index,
                ttl_ms,
                kind: "auto",
            });
        }
    }

    breakpoints
}

pub(super) fn claude_block_is_cacheable(block: &Value) -> bool {
    match block {
        Value::Null => false,
        Value::String(text) => !text.trim().is_empty(),
        Value::Object(map) => {
            if let Some(type_name) = map.get("type").and_then(Value::as_str) {
                if matches!(type_name, "thinking" | "redacted_thinking") {
                    return false;
                }
                if type_name == "text"
                    && map
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| text.trim().is_empty())
                {
                    return false;
                }
            }
            true
        }
        _ => true,
    }
}

pub(super) fn push_content_blocks(
    blocks: &mut Vec<Value>,
    kind: &str,
    index: usize,
    message: &Value,
    content_field: &str,
) {
    let Some(message_map) = message.as_object() else {
        blocks.push(json!({
            "kind": kind,
            "index": index,
            "value": message,
        }));
        return;
    };

    let mut meta = serde_json::Map::new();
    for (key, value) in message_map {
        if key != content_field {
            meta.insert(key.clone(), value.clone());
        }
    }

    match message_map.get(content_field) {
        Some(Value::Array(parts)) => {
            for (part_index, part) in parts.iter().enumerate() {
                blocks.push(json!({
                    "kind": kind,
                    "index": index,
                    "meta": Value::Object(meta.clone()),
                    "part_index": part_index,
                    "part": part,
                }));
            }
        }
        Some(part) => {
            blocks.push(json!({
                "kind": kind,
                "index": index,
                "meta": Value::Object(meta),
                "part_index": 0,
                "part": part,
            }));
        }
        None => {
            blocks.push(json!({
                "kind": kind,
                "index": index,
                "meta": Value::Object(meta),
            }));
        }
    }
}

pub(super) fn build_prefix_hashes(seed: &str, blocks: &[Value]) -> Option<Vec<String>> {
    let mut output = Vec::with_capacity(blocks.len());
    for block in blocks {
        let canonical = canonicalize_value(block);
        let bytes = serde_json::to_vec(&canonical).ok()?;
        let mut hasher = Sha256::new();
        hasher.update(seed.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
        output.push(format!("{:x}", hasher.finalize()));
    }
    Some(output)
}

pub(super) fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let mut out = serde_json::Map::new();
            for (key, value) in entries {
                let canonical = canonicalize_value(value);
                if !canonical.is_null() {
                    out.insert(key.clone(), canonical);
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_value).collect()),
        _ => value.clone(),
    }
}

pub(super) fn non_claude_candidate_indices(prefix_count: usize) -> Vec<usize> {
    if prefix_count == 0 {
        return Vec::new();
    }

    let mut indices = Vec::new();
    if prefix_count <= NON_CLAUDE_CANDIDATE_LIMIT {
        indices.extend(0..prefix_count);
    } else {
        indices.extend(0..NON_CLAUDE_CANDIDATE_HEAD);
        indices.extend(prefix_count.saturating_sub(NON_CLAUDE_CANDIDATE_TAIL)..prefix_count);
    }

    indices.sort_unstable();
    indices.dedup();
    indices.reverse();
    indices
}

pub(super) fn ttl_tag(ttl_ms: u64) -> &'static str {
    if ttl_ms == ONE_HOUR_CACHE_AFFINITY_TTL_MS {
        "1h"
    } else {
        "5m"
    }
}

pub(super) fn claude_cache_control_ttl_ms_from_value(value: &Value, default_ttl_ms: u64) -> u64 {
    match value.get("ttl").and_then(Value::as_str) {
        Some("5m") => DEFAULT_CACHE_AFFINITY_TTL_MS,
        Some("1h") => ONE_HOUR_CACHE_AFFINITY_TTL_MS,
        _ => default_ttl_ms,
    }
}

pub(super) fn claude_auto_cache_control_ttl_ms_from_value(
    value: &Value,
    default_ttl_ms: u64,
) -> u64 {
    claude_cache_control_ttl_ms_from_value(value, default_ttl_ms)
}

pub(super) fn openai_retention_tag(prompt_cache_retention: Option<&Value>) -> &'static str {
    if prompt_cache_retention
        .and_then(Value::as_str)
        .is_some_and(|value| value == "24h")
    {
        "24h"
    } else {
        "in-memory"
    }
}

pub(super) fn openai_prompt_cache_ttl_ms(prompt_cache_retention: Option<&Value>) -> u64 {
    if prompt_cache_retention
        .and_then(Value::as_str)
        .is_some_and(|value| value == "24h")
    {
        return OPENAI_24H_CACHE_AFFINITY_TTL_MS;
    }
    DEFAULT_CACHE_AFFINITY_TTL_MS
}

pub(super) fn hash_str_to_hex(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
