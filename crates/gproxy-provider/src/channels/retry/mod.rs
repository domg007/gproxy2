use std::collections::{HashMap, HashSet};
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
    /// Lightweight specificity score for this affinity key.
    /// For now it is `key.len()`. In the future we may swap this to a
    /// channel-specific metric (for example cacheable token span count).
    pub key_len: usize,
}

#[derive(Debug, Clone)]
pub struct CacheAffinityHint {
    pub candidates: Vec<CacheAffinityCandidate>,
    pub bind: CacheAffinityCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheAffinityRecord {
    channel: String,
    credential_id: i64,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
struct ScopedAffinityCandidate {
    scoped_key: String,
    ttl_ms: u64,
    key_len: usize,
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
static CACHE_AFFINITY_TRACE_ENABLED: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("GPROXY_AFFINITY_TRACE")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
});

pub enum CredentialRetryDecision<T> {
    Return(T),
    Retry {
        last_status: Option<u16>,
        last_error: Option<String>,
        last_request_meta: Option<UpstreamRequestMeta>,
    },
}

#[inline]
fn affinity_trace_enabled() -> bool {
    *CACHE_AFFINITY_TRACE_ENABLED
}

pub struct CredentialAttempt<Material> {
    pub credential_id: i64,
    pub material: Material,
    pub attempts: usize,
}

pub struct CredentialRetryContext<'a> {
    pub provider: &'a ProviderDefinition,
    pub credential_states: &'a ChannelCredentialStateStore,
    pub model: Option<&'a str>,
    pub now_unix_ms: u64,
    pub pick_mode: CredentialPickMode,
    pub cache_affinity_hint: Option<CacheAffinityHint>,
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

mod affinity;
mod selection;

#[cfg(test)]
mod tests;

pub use affinity::{
    cache_affinity_hint_from_codex_openai_response_body,
    cache_affinity_hint_from_codex_transform_request, cache_affinity_hint_from_transform_request,
    cache_affinity_protocol_from_transform_request, configured_pick_mode_uses_cache,
    credential_pick_mode,
};
pub use selection::{
    retry_with_eligible_credentials, retry_with_eligible_credentials_with_affinity,
};
