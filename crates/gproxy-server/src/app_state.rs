use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use arc_swap::ArcSwap;
use dashmap::DashMap;

use gproxy_sdk::provider::engine::GproxyEngine;
use gproxy_storage::{SeaOrmStorage, StorageWriteSender};

use crate::config::GlobalConfig;
use crate::principal::{MemoryUser, MemoryUserKey};

/// Target of a model alias lookup.
#[derive(Debug, Clone)]
pub struct ModelAliasTarget {
    pub provider_name: String,
    pub model_id: String,
}

/// In-memory model record (from models table).
#[derive(Debug, Clone)]
pub struct MemoryModel {
    pub id: i64,
    pub provider_id: i64,
    pub model_id: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub price_input_tokens: Option<f64>,
    pub price_output_tokens: Option<f64>,
    pub price_cache_read_input_tokens: Option<f64>,
    pub price_cache_creation_input_tokens: Option<f64>,
    pub price_cache_creation_input_tokens_5min: Option<f64>,
    pub price_cache_creation_input_tokens_1h: Option<f64>,
}

/// User model permission entry (whitelist).
#[derive(Debug, Clone)]
pub struct UserPermissionEntry {
    /// None = applies to all providers.
    pub provider_id: Option<i64>,
    pub model_pattern: String,
}
pub struct UserRateLimit {
    pub model_pattern: String,
    pub rpm: Option<i32>,
    pub rpd: Option<i32>,
    pub total_tokens: Option<i64>,
}

/// Central application state shared across all request handlers.
///
/// All fields use `ArcSwap` for lock-free reads and atomic hot-swapping.
/// HTTP clients live inside `GproxyEngine` — access them via `engine()`.
pub struct AppState {
    engine: ArcSwap<GproxyEngine>,
    storage: Arc<ArcSwap<SeaOrmStorage>>,
    storage_writes: StorageWriteSender,
    config: ArcSwap<GlobalConfig>,
    users: ArcSwap<Vec<MemoryUser>>,
    keys: ArcSwap<HashMap<String, MemoryUserKey>>,

    // Model registry: (provider_id, model_id) → MemoryModel
    models: ArcSwap<Vec<MemoryModel>>,
    // Model aliases: alias → target
    model_aliases: ArcSwap<HashMap<String, ModelAliasTarget>>,
    // User model permissions (whitelist): user_id → entries
    user_permissions: ArcSwap<HashMap<i64, Vec<UserPermissionEntry>>>,
    // User rate limits: user_id → limits
    user_rate_limits: ArcSwap<HashMap<i64, Vec<UserRateLimit>>>,
    // User quota tracking: user_id → (tokens_used, cost_used)
    user_quotas: ArcSwap<HashMap<i64, (i64, f64)>>,
    // RPM/RPD counters (not persisted, reset on restart)
    rate_counters: RateLimitCounters,
}

impl AppState {
    // -----------------------------------------------------------------------
    // Read (lock-free)
    // -----------------------------------------------------------------------

    pub fn engine(&self) -> Arc<GproxyEngine> {
        self.engine.load_full()
    }

    pub fn storage(&self) -> Arc<SeaOrmStorage> {
        self.storage.load_full()
    }

    pub fn storage_writes(&self) -> &StorageWriteSender {
        &self.storage_writes
    }

    pub fn config(&self) -> Arc<GlobalConfig> {
        self.config.load_full()
    }

    /// Authenticate an API key against the in-memory cache.
    /// Returns `None` if the key is not found or disabled.
    pub fn authenticate_api_key(&self, api_key: &str) -> Option<MemoryUserKey> {
        let keys = self.keys.load();
        let key = keys.get(api_key)?;
        if !key.enabled {
            return None;
        }
        // Check that the owning user is enabled
        let users = self.users.load();
        let user_enabled = users.iter().any(|u| u.id == key.user_id && u.enabled);
        if !user_enabled {
            return None;
        }
        Some(key.clone())
    }

    /// Resolve a model alias. Returns `None` if no alias is configured.
    pub fn resolve_model_alias(&self, alias: &str) -> Option<ModelAliasTarget> {
        self.model_aliases.load().get(alias).cloned()
    }

    /// Find model pricing info by model_id.
    pub fn find_model(&self, model_id: &str) -> Option<MemoryModel> {
        self.models
            .load()
            .iter()
            .find(|m| m.model_id == model_id && m.enabled)
            .cloned()
    }

    /// Check if a user is allowed to use a specific model on a provider (whitelist).
    /// Returns `false` if user has no matching permissions.
    pub fn check_model_permission(&self, user_id: i64, provider_id: i64, model: &str) -> bool {
        let perms = self.user_permissions.load();
        let Some(entries) = perms.get(&user_id) else {
            return false;
        };
        entries.iter().any(|e| {
            let provider_ok = e.provider_id.is_none() || e.provider_id == Some(provider_id);
            provider_ok && pattern_matches(&e.model_pattern, model)
        })
    }

    /// Check rate limit for a user+model request. Returns `Ok(())` if allowed,
    /// `Err(reason)` if blocked.
    pub fn check_rate_limit(&self, user_id: i64, model: &str) -> Result<(), RateLimitRejection> {
        let limits = self.user_rate_limits.load();
        let Some(user_limits) = limits.get(&user_id) else {
            return Ok(());
        };
        let matched = find_matching_limit(user_limits, model);
        let Some(limit) = matched else {
            return Ok(());
        };

        // Check RPM
        if let Some(rpm) = limit.rpm {
            let key = (user_id, model.to_string());
            let count = self.rate_counters.check_minute(&key);
            if count >= rpm as u32 {
                return Err(RateLimitRejection::Rpm { limit: rpm });
            }
        }
        // Check RPD
        if let Some(rpd) = limit.rpd {
            let key = (user_id, model.to_string());
            let count = self.rate_counters.check_day(&key);
            if count >= rpd as u32 {
                return Err(RateLimitRejection::Rpd { limit: rpd });
            }
        }
        // Check token quota
        if let Some(total_tokens) = limit.total_tokens {
            let quotas = self.user_quotas.load();
            let used = quotas.get(&user_id).map(|(t, _)| *t).unwrap_or(0);
            if used >= total_tokens {
                return Err(RateLimitRejection::TokenQuota {
                    used,
                    limit: total_tokens,
                });
            }
        }
        Ok(())
    }

    /// Record a request for rate limiting (increment RPM/RPD counters).
    pub fn record_request(&self, user_id: i64, model: &str) {
        let key = (user_id, model.to_string());
        self.rate_counters.increment_minute(&key);
        self.rate_counters.increment_day(&key);
    }

    /// Update token usage for a user (in-memory).
    pub fn add_token_usage(&self, user_id: i64, tokens: i64, cost: f64) {
        let mut quotas = (*self.user_quotas.load_full()).clone();
        let entry = quotas.entry(user_id).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(tokens);
        entry.1 += cost;
        self.user_quotas.store(Arc::new(quotas));
    }

    // -----------------------------------------------------------------------
    // Write (atomic swap)
    // -----------------------------------------------------------------------

    pub fn replace_engine(&self, engine: GproxyEngine) {
        self.engine.store(Arc::new(engine));
    }

    pub fn replace_storage(&self, storage: SeaOrmStorage) {
        self.storage.store(Arc::new(storage));
    }

    pub fn replace_config(&self, config: GlobalConfig) {
        self.config.store(Arc::new(config));
    }

    pub fn upsert_user_in_memory(&self, user: MemoryUser) {
        let mut users = (*self.users.load_full()).clone();
        if let Some(existing) = users.iter_mut().find(|u| u.id == user.id) {
            *existing = user;
        } else {
            users.push(user);
        }
        self.users.store(Arc::new(users));
    }

    pub fn remove_user_from_memory(&self, user_id: i64) {
        let mut users = (*self.users.load_full()).clone();
        users.retain(|u| u.id != user_id);
        self.users.store(Arc::new(users));
        // Also remove all keys belonging to this user
        let mut keys = (*self.keys.load_full()).clone();
        keys.retain(|_, k| k.user_id != user_id);
        self.keys.store(Arc::new(keys));
    }

    pub fn upsert_key_in_memory(&self, key: MemoryUserKey) {
        let mut keys = (*self.keys.load_full()).clone();
        // Remove old entry if key ID exists under a different api_key
        keys.retain(|_, k| k.id != key.id);
        keys.insert(key.api_key.clone(), key);
        self.keys.store(Arc::new(keys));
    }

    pub fn remove_key_from_memory(&self, key_id: i64) {
        let mut keys = (*self.keys.load_full()).clone();
        keys.retain(|_, k| k.id != key_id);
        self.keys.store(Arc::new(keys));
    }

    pub fn replace_models(&self, models: Vec<MemoryModel>) {
        self.models.store(Arc::new(models));
    }

    pub fn replace_model_aliases(&self, aliases: HashMap<String, ModelAliasTarget>) {
        self.model_aliases.store(Arc::new(aliases));
    }

    pub fn replace_user_permissions(&self, perms: HashMap<i64, Vec<UserPermissionEntry>>) {
        self.user_permissions.store(Arc::new(perms));
    }

    pub fn replace_user_rate_limits(&self, limits: HashMap<i64, Vec<UserRateLimit>>) {
        self.user_rate_limits.store(Arc::new(limits));
    }

    pub fn replace_user_quotas(&self, quotas: HashMap<i64, (i64, f64)>) {
        self.user_quotas.store(Arc::new(quotas));
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

pub struct AppStateBuilder {
    engine: Option<GproxyEngine>,
    storage: Option<Arc<SeaOrmStorage>>,
    storage_writes: Option<StorageWriteSender>,
    config: Option<GlobalConfig>,
    users: Vec<MemoryUser>,
    keys: Vec<MemoryUserKey>,
}

impl AppStateBuilder {
    pub fn new() -> Self {
        Self {
            engine: None,
            storage: None,
            storage_writes: None,
            config: None,
            users: Vec::new(),
            keys: Vec::new(),
        }
    }

    pub fn engine(mut self, engine: GproxyEngine) -> Self {
        self.engine = Some(engine);
        self
    }

    pub fn storage(mut self, storage: Arc<SeaOrmStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn storage_writes(mut self, sender: StorageWriteSender) -> Self {
        self.storage_writes = Some(sender);
        self
    }

    pub fn config(mut self, config: GlobalConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn users(mut self, users: Vec<MemoryUser>) -> Self {
        self.users = users;
        self
    }

    pub fn keys(mut self, keys: Vec<MemoryUserKey>) -> Self {
        self.keys = keys;
        self
    }

    pub fn build(self) -> AppState {
        let key_map: HashMap<String, MemoryUserKey> = self
            .keys
            .into_iter()
            .map(|k| (k.api_key.clone(), k))
            .collect();

        AppState {
            engine: ArcSwap::from_pointee(
                self.engine
                    .expect("GproxyEngine is required to build AppState"),
            ),
            storage: Arc::new(ArcSwap::from_pointee(
                (*self
                    .storage
                    .expect("SeaOrmStorage is required to build AppState"))
                .clone(),
            )),
            storage_writes: self
                .storage_writes
                .expect("StorageWriteSender is required to build AppState"),
            config: ArcSwap::from_pointee(self.config.unwrap_or_default()),
            users: ArcSwap::from_pointee(self.users),
            keys: ArcSwap::from_pointee(key_map),
            models: ArcSwap::from_pointee(Vec::new()),
            model_aliases: ArcSwap::from_pointee(HashMap::new()),
            user_permissions: ArcSwap::from_pointee(HashMap::new()),
            user_rate_limits: ArcSwap::from_pointee(HashMap::new()),
            user_quotas: ArcSwap::from_pointee(HashMap::new()),
            rate_counters: RateLimitCounters::new(),
        }
    }
}

impl Default for AppStateBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Rate limit helpers
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum RateLimitRejection {
    Rpm { limit: i32 },
    Rpd { limit: i32 },
    TokenQuota { used: i64, limit: i64 },
}

/// Sliding-window rate limit counters (in-memory, not persisted).
pub struct RateLimitCounters {
    minute: DashMap<(i64, String), WindowCounter>,
    day: DashMap<(i64, String), WindowCounter>,
}

struct WindowCounter {
    count: u32,
    window_start: Instant,
}

const MINUTE: std::time::Duration = std::time::Duration::from_secs(60);
const DAY: std::time::Duration = std::time::Duration::from_secs(86400);

impl RateLimitCounters {
    fn new() -> Self {
        Self {
            minute: DashMap::new(),
            day: DashMap::new(),
        }
    }

    fn check_minute(&self, key: &(i64, String)) -> u32 {
        self.check(&self.minute, key, MINUTE)
    }

    fn check_day(&self, key: &(i64, String)) -> u32 {
        self.check(&self.day, key, DAY)
    }

    fn increment_minute(&self, key: &(i64, String)) {
        self.increment(&self.minute, key, MINUTE);
    }

    fn increment_day(&self, key: &(i64, String)) {
        self.increment(&self.day, key, DAY);
    }

    fn check(
        &self,
        map: &DashMap<(i64, String), WindowCounter>,
        key: &(i64, String),
        window: std::time::Duration,
    ) -> u32 {
        let Some(entry) = map.get(key) else {
            return 0;
        };
        if entry.window_start.elapsed() >= window {
            0
        } else {
            entry.count
        }
    }

    fn increment(
        &self,
        map: &DashMap<(i64, String), WindowCounter>,
        key: &(i64, String),
        window: std::time::Duration,
    ) {
        let mut entry = map.entry(key.clone()).or_insert(WindowCounter {
            count: 0,
            window_start: Instant::now(),
        });
        if entry.window_start.elapsed() >= window {
            entry.count = 1;
            entry.window_start = Instant::now();
        } else {
            entry.count += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Pattern matching
// ---------------------------------------------------------------------------

/// Match a model name against a pattern.
/// - `*` matches everything
/// - `claude-*` matches anything starting with `claude-`
/// - exact string matches exactly
fn pattern_matches(pattern: &str, model: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return model.starts_with(prefix);
    }
    pattern == model
}

/// Find the most specific matching rate limit for a model.
/// Priority: exact match > prefix wildcard > `*`.
fn find_matching_limit<'a>(limits: &'a [UserRateLimit], model: &str) -> Option<&'a UserRateLimit> {
    // Exact match first
    if let Some(exact) = limits.iter().find(|l| l.model_pattern == model) {
        return Some(exact);
    }
    // Prefix wildcard (longest prefix wins)
    let mut best_prefix: Option<&UserRateLimit> = None;
    let mut best_len = 0;
    for limit in limits {
        if let Some(prefix) = limit.model_pattern.strip_suffix('*')
            && model.starts_with(prefix)
            && prefix.len() > best_len
        {
            best_prefix = Some(limit);
            best_len = prefix.len();
        }
    }
    if best_prefix.is_some() {
        return best_prefix;
    }
    // Fallback: `*`
    limits.iter().find(|l| l.model_pattern == "*")
}
