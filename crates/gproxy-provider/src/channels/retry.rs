use std::collections::HashSet;
use std::future::Future;
use std::sync::LazyLock;

use dashmap::DashMap;
use gproxy_middleware::TransformRequest;
use rand::RngExt as _;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::channels::upstream::{UpstreamError, UpstreamRequestMeta};
use crate::credential::normalize_model_cooldown_key;
use crate::{ChannelCredentialStateStore, CredentialRef, ProviderDefinition};

const DEFAULT_CACHE_AFFINITY_TTL_MS: u64 = 5 * 60 * 1000;
const ONE_HOUR_CACHE_AFFINITY_TTL_MS: u64 = 60 * 60 * 1000;
const OPENAI_24H_CACHE_AFFINITY_TTL_MS: u64 = 24 * 60 * 60 * 1000;
const GEMINI_CACHED_CONTENT_TTL_MS: u64 = 60 * 60 * 1000;
const NON_CLAUDE_CANDIDATE_LIMIT: usize = 64;
const NON_CLAUDE_CANDIDATE_HEAD: usize = 8;
const NON_CLAUDE_CANDIDATE_TAIL: usize = 56;
const CLAUDE_BREAKPOINT_LOOKBACK: usize = 20;

#[derive(Debug, Clone)]
pub struct CacheAffinityCandidate {
    pub key: String,
    pub ttl_ms: u64,
}

#[derive(Debug, Clone)]
pub struct CacheAffinityHint {
    pub candidates: Vec<CacheAffinityCandidate>,
    pub bind: CacheAffinityCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheAffinityRecord {
    credential_id: i64,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
struct ScopedAffinityCandidate {
    scoped_key: String,
    ttl_ms: u64,
}

#[derive(Debug, Clone)]
struct ClaudeCacheBlock {
    hash_value: Value,
    explicit_ttl_ms: Option<u64>,
    cacheable: bool,
}

#[derive(Debug, Clone)]
struct ClaudeBreakpoint {
    index: usize,
    ttl_ms: u64,
    kind: &'static str,
}

static CACHE_AFFINITY: LazyLock<DashMap<String, CacheAffinityRecord>> = LazyLock::new(DashMap::new);

pub enum CredentialRetryDecision<T> {
    Return(T),
    Retry {
        last_status: Option<u16>,
        last_error: Option<String>,
        last_request_meta: Option<UpstreamRequestMeta>,
    },
}

pub struct CredentialAttempt<Material> {
    pub credential_id: i64,
    pub material: Material,
    pub attempts: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialPickMode {
    StickyNoCache,
    RoundRobinWithCache,
    RoundRobinNoCache,
}

struct CredentialCandidate<Material> {
    credential_id: i64,
    material: Material,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheAffinityProtocol {
    OpenAiResponses,
    OpenAiChatCompletions,
    ClaudeMessages,
    GeminiGenerateContent,
}

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

pub fn cache_affinity_hint_for_claude_effective_body(
    body_json: Value,
) -> Option<CacheAffinityHint> {
    cache_affinity_hint_for_claude(body_json)
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

#[allow(clippy::too_many_arguments)]
pub async fn retry_with_eligible_credentials_with_affinity<Material, T, Select, Attempt, Fut>(
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    model: Option<&str>,
    now_unix_ms: u64,
    pick_mode: CredentialPickMode,
    cache_affinity_hint: Option<CacheAffinityHint>,
    mut select_material: Select,
    mut attempt: Attempt,
) -> Result<T, UpstreamError>
where
    Select: FnMut(&CredentialRef) -> Option<Material>,
    Attempt: FnMut(CredentialAttempt<Material>) -> Fut,
    Fut: Future<Output = CredentialRetryDecision<T>>,
{
    let normalized_model = model.and_then(normalize_model_cooldown_key);
    let mut remaining = credential_states
        .eligible_credentials(
            &provider.channel,
            provider.credentials.list_credentials(),
            normalized_model.as_deref(),
            now_unix_ms,
        )
        .into_iter()
        .filter_map(|credential| {
            select_material(credential).map(|material| CredentialCandidate {
                credential_id: credential.id,
                material,
            })
        })
        .collect::<Vec<_>>();

    if remaining.is_empty() {
        return Err(UpstreamError::NoEligibleCredential {
            channel: provider.channel.as_str().to_string(),
            model: normalized_model,
        });
    }

    let use_cache_affinity = matches!(pick_mode, CredentialPickMode::RoundRobinWithCache);
    let scoped_candidates = if use_cache_affinity {
        cache_affinity_hint
            .as_ref()
            .map(|hint| scoped_affinity_candidates(provider, hint))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let scoped_bind = if use_cache_affinity {
        cache_affinity_hint
            .as_ref()
            .map(|hint| ScopedAffinityCandidate {
                scoped_key: scoped_affinity_key(provider, hint.bind.key.as_str()),
                ttl_ms: hint.bind.ttl_ms,
            })
    } else {
        None
    };

    let mut attempts = 0usize;
    let mut last_credential_id = None;
    let mut last_status = None;
    let mut last_error = None;
    let mut last_request_meta = None;

    while !remaining.is_empty() {
        let (idx, matched_affinity_idx) =
            pick_candidate_index(&remaining, &scoped_candidates, now_unix_ms, pick_mode);
        let candidate = remaining.swap_remove(idx);
        attempts += 1;
        match attempt(CredentialAttempt {
            credential_id: candidate.credential_id,
            material: candidate.material,
            attempts,
        })
        .await
        {
            CredentialRetryDecision::Return(value) => {
                if use_cache_affinity {
                    if let Some(bind) = scoped_bind.as_ref() {
                        bind_affinity(
                            bind.scoped_key.as_str(),
                            candidate.credential_id,
                            now_unix_ms.saturating_add(bind.ttl_ms),
                        );
                    }
                    if let Some(matched_idx) = matched_affinity_idx
                        && let Some(hit) = scoped_candidates.get(matched_idx)
                    {
                        bind_affinity(
                            hit.scoped_key.as_str(),
                            candidate.credential_id,
                            now_unix_ms.saturating_add(hit.ttl_ms),
                        );
                    }
                }
                return Ok(value);
            }
            CredentialRetryDecision::Retry {
                last_status: status,
                last_error: error,
                last_request_meta: request_meta,
            } => {
                if use_cache_affinity
                    && let Some(matched_idx) = matched_affinity_idx
                    && let Some(hit) = scoped_candidates.get(matched_idx)
                {
                    clear_affinity(hit.scoped_key.as_str());
                }
                last_credential_id = Some(candidate.credential_id);
                last_status = status;
                last_error = error;
                last_request_meta = request_meta;
            }
        }
    }

    Err(UpstreamError::AllCredentialsExhausted {
        channel: provider.channel.as_str().to_string(),
        attempts,
        last_credential_id,
        last_status,
        last_error,
        last_request_meta: last_request_meta.map(Box::new),
    })
}

pub async fn retry_with_eligible_credentials<Material, T, Select, Attempt, Fut>(
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    model: Option<&str>,
    now_unix_ms: u64,
    select_material: Select,
    attempt: Attempt,
) -> Result<T, UpstreamError>
where
    Select: FnMut(&CredentialRef) -> Option<Material>,
    Attempt: FnMut(CredentialAttempt<Material>) -> Fut,
    Fut: Future<Output = CredentialRetryDecision<T>>,
{
    retry_with_eligible_credentials_with_affinity(
        provider,
        credential_states,
        model,
        now_unix_ms,
        CredentialPickMode::RoundRobinNoCache,
        None,
        select_material,
        attempt,
    )
    .await
}

fn scoped_affinity_key(provider: &ProviderDefinition, key: &str) -> String {
    format!("{}::{key}", provider.channel.as_str())
}

fn scoped_affinity_candidates(
    provider: &ProviderDefinition,
    hint: &CacheAffinityHint,
) -> Vec<ScopedAffinityCandidate> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::with_capacity(hint.candidates.len());
    for candidate in &hint.candidates {
        let scoped_key = scoped_affinity_key(provider, candidate.key.as_str());
        if seen.insert(scoped_key.clone()) {
            candidates.push(ScopedAffinityCandidate {
                scoped_key,
                ttl_ms: candidate.ttl_ms,
            });
        }
    }
    candidates
}

fn pick_candidate_index<Material>(
    remaining: &[CredentialCandidate<Material>],
    scoped_candidates: &[ScopedAffinityCandidate],
    now_unix_ms: u64,
    pick_mode: CredentialPickMode,
) -> (usize, Option<usize>) {
    if matches!(pick_mode, CredentialPickMode::RoundRobinWithCache) {
        for (candidate_idx, candidate) in scoped_candidates.iter().enumerate() {
            if let Some(credential_id) =
                get_affinity_credential_id(candidate.scoped_key.as_str(), now_unix_ms)
                && let Some(idx) = remaining
                    .iter()
                    .position(|item| item.credential_id == credential_id)
            {
                return (idx, Some(candidate_idx));
            }
        }
        return (rand::rng().random_range(0..remaining.len()), None);
    }

    if matches!(pick_mode, CredentialPickMode::RoundRobinNoCache) {
        return (rand::rng().random_range(0..remaining.len()), None);
    }

    let idx = remaining
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| candidate.credential_id)
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    (idx, None)
}

fn get_affinity_credential_id(scoped_key: &str, now_unix_ms: u64) -> Option<i64> {
    let record = CACHE_AFFINITY.get(scoped_key)?;
    if record.expires_at_unix_ms <= now_unix_ms {
        drop(record);
        CACHE_AFFINITY.remove(scoped_key);
        return None;
    }
    Some(record.credential_id)
}

fn bind_affinity(scoped_key: &str, credential_id: i64, expires_at_unix_ms: u64) {
    CACHE_AFFINITY.insert(
        scoped_key.to_string(),
        CacheAffinityRecord {
            credential_id,
            expires_at_unix_ms,
        },
    );
}

fn clear_affinity(scoped_key: &str) {
    CACHE_AFFINITY.remove(scoped_key);
}

fn cache_affinity_hint_for_openai_responses(body_json: Value) -> Option<CacheAffinityHint> {
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

fn cache_affinity_hint_for_openai_chat(body_json: Value) -> Option<CacheAffinityHint> {
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

fn cache_affinity_hint_for_claude(body_json: Value) -> Option<CacheAffinityHint> {
    let blocks = claude_cache_blocks(&body_json);
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

    let mut breakpoints = claude_breakpoints(&body_json, &blocks);
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
                candidates.push(CacheAffinityCandidate {
                    key,
                    ttl_ms: breakpoint.ttl_ms,
                });
            }
        }
    }

    let bind = candidates.first()?.clone();
    Some(CacheAffinityHint { candidates, bind })
}

fn cache_affinity_hint_for_gemini(model: &str, body_json: Value) -> Option<CacheAffinityHint> {
    if let Some(cached_content) = body_json
        .get("cachedContent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let key = format!("gemini.cachedContent:{}", hash_str_to_hex(cached_content));
        let candidate = CacheAffinityCandidate {
            key,
            ttl_ms: GEMINI_CACHED_CONTENT_TTL_MS,
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

fn non_claude_affinity_hint<F>(
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
        candidates.push(CacheAffinityCandidate {
            key: key_builder(prefix_hash),
            ttl_ms,
        });
    }

    if candidates.is_empty() {
        return None;
    }

    let bind = CacheAffinityCandidate {
        key: key_builder(bind_hash),
        ttl_ms,
    };

    Some(CacheAffinityHint { candidates, bind })
}

fn openai_chat_cache_blocks(body_json: &Value) -> Vec<Value> {
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

fn openai_responses_cache_blocks(body_json: &Value) -> Vec<Value> {
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

fn gemini_cache_blocks(body_json: &Value) -> Vec<Value> {
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

fn claude_cache_blocks(body_json: &Value) -> Vec<ClaudeCacheBlock> {
    let mut blocks = Vec::new();

    if let Some(tools) = body_json.get("tools").and_then(Value::as_array) {
        for (tool_index, tool) in tools.iter().enumerate() {
            let explicit_ttl_ms = tool
                .get("cache_control")
                .map(claude_cache_control_ttl_ms_from_value);
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
                        .map(claude_cache_control_ttl_ms_from_value);
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
                        let explicit_ttl_ms = item
                            .get("cache_control")
                            .map(claude_cache_control_ttl_ms_from_value);
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

fn claude_breakpoints(body_json: &Value, blocks: &[ClaudeCacheBlock]) -> Vec<ClaudeBreakpoint> {
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
        let ttl_ms = claude_auto_cache_control_ttl_ms_from_value(cache_control);
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

fn claude_block_is_cacheable(block: &Value) -> bool {
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

fn push_content_blocks(
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

fn build_prefix_hashes(seed: &str, blocks: &[Value]) -> Option<Vec<String>> {
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());

    let mut output = Vec::with_capacity(blocks.len());
    for block in blocks {
        let canonical = canonicalize_value(block);
        let bytes = serde_json::to_vec(&canonical).ok()?;
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
        output.push(format!("{:x}", hasher.clone().finalize()));
    }
    Some(output)
}

fn canonicalize_value(value: &Value) -> Value {
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

fn non_claude_candidate_indices(prefix_count: usize) -> Vec<usize> {
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

fn ttl_tag(ttl_ms: u64) -> &'static str {
    if ttl_ms == ONE_HOUR_CACHE_AFFINITY_TTL_MS {
        "1h"
    } else {
        "5m"
    }
}

fn claude_cache_control_ttl_ms_from_value(value: &Value) -> u64 {
    if value
        .get("ttl")
        .and_then(Value::as_str)
        .is_some_and(|ttl| ttl == "1h")
    {
        return ONE_HOUR_CACHE_AFFINITY_TTL_MS;
    }
    DEFAULT_CACHE_AFFINITY_TTL_MS
}

fn claude_auto_cache_control_ttl_ms_from_value(value: &Value) -> u64 {
    if value
        .get("ttl")
        .and_then(Value::as_str)
        .is_some_and(|ttl| ttl == "1h")
    {
        return ONE_HOUR_CACHE_AFFINITY_TTL_MS;
    }
    if value
        .get("ttl")
        .and_then(Value::as_str)
        .is_some_and(|ttl| ttl == "5m")
    {
        return DEFAULT_CACHE_AFFINITY_TTL_MS;
    }
    DEFAULT_CACHE_AFFINITY_TTL_MS
}

fn openai_retention_tag(prompt_cache_retention: Option<&Value>) -> &'static str {
    if prompt_cache_retention
        .and_then(Value::as_str)
        .is_some_and(|value| value == "24h")
    {
        "24h"
    } else {
        "in-memory"
    }
}

fn openai_prompt_cache_ttl_ms(prompt_cache_retention: Option<&Value>) -> u64 {
    if prompt_cache_retention
        .and_then(Value::as_str)
        .is_some_and(|value| value == "24h")
    {
        return OPENAI_24H_CACHE_AFFINITY_TTL_MS;
    }
    DEFAULT_CACHE_AFFINITY_TTL_MS
}

fn hash_str_to_hex(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        build_prefix_hashes, cache_affinity_hint_for_claude_effective_body,
        cache_affinity_hint_for_gemini, cache_affinity_hint_for_openai_chat,
        cache_affinity_hint_for_openai_responses, hash_str_to_hex, non_claude_candidate_indices,
        openai_chat_cache_blocks,
    };

    #[test]
    fn openai_chat_ignores_stream_and_sampling_for_affinity() {
        let body_a = json!({
            "model": "gpt-5",
            "prompt_cache_key": "k1",
            "stream": false,
            "temperature": 0.1,
            "max_tokens": 200,
            "tools": [{"type":"function","function":{"name":"f"}}],
            "messages": [{"role":"user","content":"hello"}],
        });
        let body_b = json!({
            "model": "gpt-5",
            "prompt_cache_key": "k1",
            "stream": true,
            "temperature": 0.9,
            "max_tokens": 999,
            "tools": [{"type":"function","function":{"name":"f"}}],
            "messages": [{"role":"user","content":"hello"}],
        });

        let hint_a = cache_affinity_hint_for_openai_chat(body_a).expect("hint a");
        let hint_b = cache_affinity_hint_for_openai_chat(body_b).expect("hint b");
        assert_eq!(hint_a.bind.key, hint_b.bind.key);
    }

    #[test]
    fn openai_responses_ignores_stream_and_output_tokens_for_affinity() {
        let body_a = json!({
            "model": "gpt-5",
            "stream": false,
            "max_output_tokens": 128,
            "input": [{"role":"user","content":[{"type":"input_text","text":"hello"}]}],
        });
        let body_b = json!({
            "model": "gpt-5",
            "stream": true,
            "max_output_tokens": 4096,
            "input": [{"role":"user","content":[{"type":"input_text","text":"hello"}]}],
        });

        let hint_a = cache_affinity_hint_for_openai_responses(body_a).expect("hint a");
        let hint_b = cache_affinity_hint_for_openai_responses(body_b).expect("hint b");
        assert_eq!(hint_a.bind.key, hint_b.bind.key);
    }

    #[test]
    fn claude_without_breakpoints_returns_none() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [{"role":"user","content":"hello"}]
        });
        assert!(cache_affinity_hint_for_claude_effective_body(body).is_none());
    }

    #[test]
    fn claude_top_level_cache_control_creates_auto_breakpoint() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "cache_control": {"type":"ephemeral", "ttl":"1h"},
            "messages": [{"role":"user","content":"hello"}]
        });
        let hint = cache_affinity_hint_for_claude_effective_body(body).expect("hint");
        assert!(hint.bind.key.contains("bp=auto"));
        assert!(hint.bind.key.contains("ttl=1h"));
    }

    #[test]
    fn claude_top_level_cache_control_without_ttl_defaults_to_5m() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "cache_control": {"type":"ephemeral"},
            "messages": [{"role":"user","content":"hello"}]
        });
        let hint = cache_affinity_hint_for_claude_effective_body(body).expect("hint");
        assert!(hint.bind.key.contains("bp=auto"));
        assert!(hint.bind.key.contains("ttl=5m"));
    }

    #[test]
    fn claude_explicit_breakpoint_creates_candidates() {
        let body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [{
                "role":"user",
                "content":[{"type":"text","text":"hello","cache_control":{"type":"ephemeral"}}]
            }]
        });
        let hint = cache_affinity_hint_for_claude_effective_body(body).expect("hint");
        assert!(!hint.candidates.is_empty());
        assert!(hint.bind.key.contains("bp=explicit"));
    }

    #[test]
    fn gemini_cached_content_uses_strong_key() {
        let body = json!({
            "cachedContent": "cachedContents/abc",
            "contents": [{"role":"user","parts":[{"text":"hello"}]}]
        });
        let hint = cache_affinity_hint_for_gemini("models/gemini-2.5-pro", body).expect("hint");
        assert!(hint.bind.key.starts_with("gemini.cachedContent:"));
        assert_eq!(hint.candidates.len(), 1);
    }

    #[test]
    fn gemini_prefix_mode_when_no_cached_content() {
        let body = json!({
            "systemInstruction": {"role":"system","parts":[{"text":"s"}]},
            "contents": [{"role":"user","parts":[{"text":"hello"}]}]
        });
        let hint = cache_affinity_hint_for_gemini("models/gemini-2.5-pro", body).expect("hint");
        assert!(hint.bind.key.starts_with("gemini.generateContent:prefix:"));
        assert!(!hint.candidates.is_empty());
    }

    #[test]
    fn non_claude_candidate_sampling_prefers_tail_when_prefixes_exceed_limit() {
        let messages = (0..80)
            .map(|idx| {
                json!({
                    "role": "user",
                    "content": format!("msg-{idx}")
                })
            })
            .collect::<Vec<_>>();
        let body = json!({
            "model": "gpt-5",
            "prompt_cache_key": "sample-key",
            "messages": messages,
        });

        let hint = cache_affinity_hint_for_openai_chat(body.clone()).expect("hint");
        assert_eq!(hint.candidates.len(), 64);
        assert_eq!(
            hint.candidates.first().map(|v| &v.key),
            Some(&hint.bind.key)
        );

        let blocks = openai_chat_cache_blocks(&body);
        let prefix_hashes =
            build_prefix_hashes("openai.chat:gpt-5", &blocks).expect("prefix hashes");
        let sampled = non_claude_candidate_indices(prefix_hashes.len());
        assert_eq!(sampled.len(), 64);
        assert_eq!(sampled[0], 79);
        assert_eq!(sampled[55], 24);
        assert_eq!(sampled[56], 7);
        assert_eq!(sampled[63], 0);

        let prompt_cache_key_hash = hash_str_to_hex("sample-key");
        let key_for_index = |idx: usize| {
            format!(
                "openai.chat:ret=in-memory:k={prompt_cache_key_hash}:h={}",
                prefix_hashes[idx]
            )
        };

        assert_eq!(hint.candidates[55].key, key_for_index(24));
        assert_eq!(hint.candidates[56].key, key_for_index(7));
        assert_eq!(hint.candidates[63].key, key_for_index(0));
    }
}
