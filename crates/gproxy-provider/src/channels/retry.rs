use std::future::Future;
use std::sync::LazyLock;

use dashmap::DashMap;
use gproxy_middleware::TransformRequest;
use rand::RngExt as _;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::channels::upstream::{UpstreamError, UpstreamRequestMeta};
use crate::credential::normalize_model_cooldown_key;
use crate::{ChannelCredentialStateStore, CredentialRef, ProviderDefinition};

const DEFAULT_CACHE_AFFINITY_TTL_MS: u64 = 5 * 60 * 1000;
const ONE_HOUR_CACHE_AFFINITY_TTL_MS: u64 = 65 * 60 * 1000;
const OPENAI_24H_CACHE_AFFINITY_TTL_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
pub struct CacheAffinityHint {
    pub key: String,
    pub ttl_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheAffinityRecord {
    credential_id: i64,
    expires_at_unix_ms: u64,
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
    StickyWithCache,
    RoundRobinWithCache,
    RoundRobinNoCache,
}

struct CredentialCandidate<Material> {
    credential_id: i64,
    material: Material,
}

pub fn cache_affinity_hint_from_transform_request(
    request: &TransformRequest,
) -> Option<CacheAffinityHint> {
    match request {
        TransformRequest::GenerateContentOpenAiResponse(value)
        | TransformRequest::StreamGenerateContentOpenAiResponse(value) => {
            cache_affinity_hint_for_openai_responses(serde_json::to_value(&value.body).ok()?)
        }
        TransformRequest::GenerateContentOpenAiChatCompletions(value)
        | TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
            cache_affinity_hint_for_openai_chat(serde_json::to_value(&value.body).ok()?)
        }
        TransformRequest::GenerateContentClaude(value)
        | TransformRequest::StreamGenerateContentClaude(value) => {
            cache_affinity_hint_for_claude(serde_json::to_value(&value.body).ok()?)
        }
        TransformRequest::GenerateContentGemini(value) => cache_affinity_hint_for_gemini(
            &value.path.model,
            serde_json::to_value(&value.body).ok()?,
        ),
        TransformRequest::StreamGenerateContentGeminiSse(value)
        | TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
            cache_affinity_hint_for_gemini(
                &value.path.model,
                serde_json::to_value(&value.body).ok()?,
            )
        }
        _ => None,
    }
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
        CredentialPickMode::StickyWithCache => {
            if cache_affinity_hint.is_some() {
                CredentialPickMode::StickyWithCache
            } else {
                CredentialPickMode::StickyNoCache
            }
        }
        CredentialPickMode::RoundRobinNoCache => CredentialPickMode::RoundRobinNoCache,
        CredentialPickMode::StickyNoCache => CredentialPickMode::StickyNoCache,
    }
}

pub fn configured_pick_mode_uses_cache(configured_pick_mode: CredentialPickMode) -> bool {
    matches!(
        configured_pick_mode,
        CredentialPickMode::RoundRobinWithCache | CredentialPickMode::StickyWithCache
    )
}

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

    let use_cache_affinity = matches!(
        pick_mode,
        CredentialPickMode::RoundRobinWithCache | CredentialPickMode::StickyWithCache
    );
    let scoped_affinity_key = if use_cache_affinity {
        cache_affinity_hint
            .as_ref()
            .map(|hint| scoped_affinity_key(provider, hint.key.as_str()))
    } else {
        None
    };
    let affinity_ttl_ms = cache_affinity_hint
        .as_ref()
        .map(|hint| hint.ttl_ms)
        .unwrap_or(DEFAULT_CACHE_AFFINITY_TTL_MS);

    let mut attempts = 0usize;
    let mut last_credential_id = None;
    let mut last_status = None;
    let mut last_error = None;
    let mut last_request_meta = None;

    while !remaining.is_empty() {
        let (idx, picked_from_affinity) = pick_candidate_index(
            &remaining,
            scoped_affinity_key.as_deref(),
            now_unix_ms,
            pick_mode,
        );
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
                if use_cache_affinity && let Some(key) = scoped_affinity_key.as_deref() {
                    bind_affinity(
                        key,
                        candidate.credential_id,
                        now_unix_ms.saturating_add(affinity_ttl_ms),
                    );
                }
                return Ok(value);
            }
            CredentialRetryDecision::Retry {
                last_status: status,
                last_error: error,
                last_request_meta: request_meta,
            } => {
                if use_cache_affinity
                    && picked_from_affinity
                    && let Some(key) = scoped_affinity_key.as_deref()
                {
                    clear_affinity(key);
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

fn pick_candidate_index<Material>(
    remaining: &[CredentialCandidate<Material>],
    scoped_affinity_key: Option<&str>,
    now_unix_ms: u64,
    pick_mode: CredentialPickMode,
) -> (usize, bool) {
    if matches!(
        pick_mode,
        CredentialPickMode::RoundRobinWithCache | CredentialPickMode::StickyWithCache
    ) {
        if let Some(key) = scoped_affinity_key
            && let Some(credential_id) = get_affinity_credential_id(key, now_unix_ms)
            && let Some(idx) = remaining
                .iter()
                .position(|candidate| candidate.credential_id == credential_id)
        {
            return (idx, true);
        }
    }

    if matches!(pick_mode, CredentialPickMode::RoundRobinWithCache) {
        return (rand::rng().random_range(0..remaining.len()), false);
    }

    if matches!(pick_mode, CredentialPickMode::RoundRobinNoCache) {
        return (rand::rng().random_range(0..remaining.len()), false);
    }

    let idx = remaining
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| candidate.credential_id)
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    (idx, false)
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
    if let Some(key) = body_json
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(CacheAffinityHint {
            key: format!("openai.responses:key:{key}"),
            ttl_ms,
        });
    }
    let hash = hash_to_hex(body_json)?;
    Some(CacheAffinityHint {
        key: format!("openai.responses:hash:{hash}"),
        ttl_ms,
    })
}

fn cache_affinity_hint_for_openai_chat(body_json: Value) -> Option<CacheAffinityHint> {
    let ttl_ms = openai_prompt_cache_ttl_ms(body_json.get("prompt_cache_retention"));
    if let Some(key) = body_json
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(CacheAffinityHint {
            key: format!("openai.chat:key:{key}"),
            ttl_ms,
        });
    }
    let hash = hash_to_hex(body_json)?;
    Some(CacheAffinityHint {
        key: format!("openai.chat:hash:{hash}"),
        ttl_ms,
    })
}

fn cache_affinity_hint_for_claude(body_json: Value) -> Option<CacheAffinityHint> {
    let ttl_ms = claude_cache_ttl_ms(body_json.get("cache_control"));
    let hash = hash_to_hex(body_json)?;
    Some(CacheAffinityHint {
        key: format!("claude.messages:hash:{hash}"),
        ttl_ms,
    })
}

fn cache_affinity_hint_for_gemini(model: &str, body_json: Value) -> Option<CacheAffinityHint> {
    if let Some(cached_content) = body_json
        .get("cachedContent")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(CacheAffinityHint {
            key: format!("gemini.cachedContent:{cached_content}"),
            ttl_ms: DEFAULT_CACHE_AFFINITY_TTL_MS,
        });
    }
    let hash = hash_to_hex(json!({
        "model": model,
        "body": body_json,
    }))?;
    Some(CacheAffinityHint {
        key: format!("gemini.generateContent:hash:{hash}"),
        ttl_ms: DEFAULT_CACHE_AFFINITY_TTL_MS,
    })
}

fn claude_cache_ttl_ms(cache_control: Option<&Value>) -> u64 {
    if cache_control
        .and_then(|value| value.get("ttl"))
        .and_then(Value::as_str)
        .is_some_and(|ttl| ttl == "1h")
    {
        return ONE_HOUR_CACHE_AFFINITY_TTL_MS;
    }
    DEFAULT_CACHE_AFFINITY_TTL_MS
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

fn hash_to_hex<T: Serialize>(value: T) -> Option<String> {
    let bytes = serde_json::to_vec(&value).ok()?;
    let digest = Sha256::digest(&bytes);
    Some(format!("{digest:x}"))
}
