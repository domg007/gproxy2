use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use dashmap::DashMap;

use gproxy_sdk::provider::engine::GproxyEngine;
use gproxy_storage::{SeaOrmStorage, StorageWriteSender};

use crate::config::GlobalConfig;
use crate::middleware::model_alias::ModelAliasTarget;
use crate::middleware::permission::{self, PermissionEntry};
use crate::middleware::rate_limit::{
    RateLimitCounters, RateLimitRejection, RateLimitRule, find_matching_rule,
};
use crate::principal::{MemoryUser, MemoryUserKey};

// Re-export middleware types
pub use crate::middleware::model_alias::ModelAliasTarget as ModelAliasTargetExport;
pub use crate::middleware::permission::PermissionEntry as PermissionEntryExport;
pub use crate::middleware::rate_limit::{
    RateLimitRejection as RateLimitRejectionExport, RateLimitRule as RateLimitRuleExport,
};

/// A price tier based on input_tokens threshold.
///
/// When `input_tokens` in usage falls within this tier's range,
/// all token types use this tier's prices.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PriceTier {
    /// Upper bound of input_tokens for this tier (exclusive).
    /// Use `i64::MAX` or omit for the last tier.
    pub input_tokens_up_to: i64,
    pub price_input_tokens: Option<f64>,
    pub price_output_tokens: Option<f64>,
    pub price_cache_read_input_tokens: Option<f64>,
    pub price_cache_creation_input_tokens: Option<f64>,
    pub price_cache_creation_input_tokens_5min: Option<f64>,
    pub price_cache_creation_input_tokens_1h: Option<f64>,
}

/// In-memory model record (from models table).
#[derive(Debug, Clone)]
pub struct MemoryModel {
    pub id: i64,
    pub provider_id: i64,
    pub model_id: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub price_each_call: Option<f64>,
    /// Tiered pricing: the first tier whose `input_tokens_up_to`
    /// exceeds the request's input_tokens is used for per-token prices.
    /// Sorted by `input_tokens_up_to` ascending.
    pub price_tiers: Vec<PriceTier>,
}

/// Central application state shared across all request handlers.
pub struct AppState {
    engine: ArcSwap<GproxyEngine>,
    storage: Arc<ArcSwap<SeaOrmStorage>>,
    storage_writes: StorageWriteSender,
    config: ArcSwap<GlobalConfig>,
    users: ArcSwap<Vec<MemoryUser>>,
    keys: ArcSwap<HashMap<String, MemoryUserKey>>,
    models: ArcSwap<Vec<MemoryModel>>,
    model_aliases: ArcSwap<HashMap<String, ModelAliasTarget>>,
    provider_names: ArcSwap<HashMap<String, i64>>,
    user_permissions: ArcSwap<HashMap<i64, Vec<PermissionEntry>>>,
    user_rate_limits: ArcSwap<HashMap<i64, Vec<RateLimitRule>>>,
    user_quotas: DashMap<i64, (f64, f64)>,
    pub rate_counters: RateLimitCounters,
}

impl AppState {
    // -----------------------------------------------------------------------
    // Read
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

    pub fn authenticate_api_key(&self, api_key: &str) -> Option<MemoryUserKey> {
        let keys = self.keys.load();
        let key = keys.get(api_key)?;
        if !key.enabled {
            return None;
        }
        let users = self.users.load();
        if !users.iter().any(|u| u.id == key.user_id && u.enabled) {
            return None;
        }
        Some(key.clone())
    }

    /// Get all keys for a user (from memory).
    pub fn keys_for_user(&self, user_id: i64) -> Vec<MemoryUserKey> {
        let keys = self.keys.load();
        keys.values()
            .filter(|k| k.user_id == user_id)
            .cloned()
            .collect()
    }

    /// Get all users (from memory).
    pub fn users_snapshot(&self) -> Arc<Vec<MemoryUser>> {
        self.users.load_full()
    }

    /// Get all keys (from memory).
    pub fn keys_snapshot(&self) -> Arc<HashMap<String, MemoryUserKey>> {
        self.keys.load_full()
    }

    pub fn find_model(&self, model_id: &str) -> Option<MemoryModel> {
        self.models
            .load()
            .iter()
            .find(|m| m.model_id == model_id && m.enabled)
            .cloned()
    }

    /// Get user quota info: (quota, cost_used). Returns (0, 0) if not set.
    pub fn get_user_quota(&self, user_id: i64) -> (f64, f64) {
        self.user_quotas
            .get(&user_id)
            .map(|e| *e.value())
            .unwrap_or((0.0, 0.0))
    }

    pub fn resolve_model_alias(&self, alias: &str) -> Option<ModelAliasTarget> {
        self.model_aliases.load().get(alias).cloned()
    }

    pub fn check_model_permission(&self, user_id: i64, provider_name: &str, model: &str) -> bool {
        let provider_id = self
            .provider_names
            .load()
            .get(provider_name)
            .copied()
            .unwrap_or(0);
        let perms = self.user_permissions.load();
        let Some(entries) = perms.get(&user_id) else {
            return false;
        };
        entries.iter().any(|e| {
            let provider_ok = e.provider_id.is_none() || e.provider_id == Some(provider_id);
            provider_ok && permission::pattern_matches(&e.model_pattern, model)
        })
    }

    pub fn check_rate_limit(&self, user_id: i64, model: &str) -> Result<(), RateLimitRejection> {
        let limits = self.user_rate_limits.load();
        let Some(user_limits) = limits.get(&user_id) else {
            return Ok(());
        };
        let Some(rule) = find_matching_rule(user_limits, model) else {
            return Ok(());
        };
        if let Some(rpm) = rule.rpm
            && self.rate_counters.check_rpm(user_id, model) >= rpm as u32
        {
            return Err(RateLimitRejection::Rpm { limit: rpm });
        }
        if let Some(rpd) = rule.rpd
            && self.rate_counters.check_rpd(user_id, model) >= rpd as u32
        {
            return Err(RateLimitRejection::Rpd { limit: rpd });
        }
        // Check cost quota
        let (quota, cost_used) = self.get_user_quota(user_id);
        if quota > 0.0 && cost_used >= quota {
            return Err(RateLimitRejection::QuotaExhausted { quota, cost_used });
        }
        Ok(())
    }

    pub fn record_request(&self, user_id: i64, model: &str) {
        self.rate_counters.check_and_increment(user_id, model);
    }

    /// Atomically add cost to a user's quota usage. Returns (quota, new_cost_used).
    pub fn add_cost_usage(&self, user_id: i64, cost: f64) -> (f64, f64) {
        let mut entry = self.user_quotas.entry(user_id).or_insert((0.0, 0.0));
        entry.1 += cost;
        *entry.value()
    }

    // -----------------------------------------------------------------------
    // Write
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
        let mut keys = (*self.keys.load_full()).clone();
        keys.retain(|_, k| k.user_id != user_id);
        self.keys.store(Arc::new(keys));
    }

    pub fn upsert_key_in_memory(&self, key: MemoryUserKey) {
        let mut keys = (*self.keys.load_full()).clone();
        keys.retain(|_, k| k.id != key.id);
        keys.insert(key.api_key.clone(), key);
        self.keys.store(Arc::new(keys));
    }

    pub fn remove_key_from_memory(&self, key_id: i64) {
        let mut keys = (*self.keys.load_full()).clone();
        keys.retain(|_, k| k.id != key_id);
        self.keys.store(Arc::new(keys));
    }

    pub fn replace_users(&self, users: Vec<MemoryUser>) {
        self.users.store(Arc::new(users));
    }

    pub fn replace_keys(&self, keys: Vec<MemoryUserKey>) {
        let map: HashMap<String, MemoryUserKey> =
            keys.into_iter().map(|k| (k.api_key.clone(), k)).collect();
        self.keys.store(Arc::new(map));
    }

    pub fn replace_models(&self, models: Vec<MemoryModel>) {
        self.models.store(Arc::new(models));
    }

    pub fn replace_model_aliases(&self, aliases: HashMap<String, ModelAliasTarget>) {
        self.model_aliases.store(Arc::new(aliases));
    }

    pub fn replace_provider_names(&self, names: HashMap<String, i64>) {
        self.provider_names.store(Arc::new(names));
    }

    pub fn replace_user_permissions(&self, perms: HashMap<i64, Vec<PermissionEntry>>) {
        self.user_permissions.store(Arc::new(perms));
    }

    pub fn replace_user_rate_limits(&self, limits: HashMap<i64, Vec<RateLimitRule>>) {
        self.user_rate_limits.store(Arc::new(limits));
    }

    pub fn user_quotas_snapshot(&self) -> HashMap<i64, (f64, f64)> {
        self.user_quotas
            .iter()
            .map(|e| (*e.key(), *e.value()))
            .collect()
    }

    pub fn replace_user_quotas(&self, quotas: HashMap<i64, (f64, f64)>) {
        self.user_quotas.clear();
        for (k, v) in quotas {
            self.user_quotas.insert(k, v);
        }
    }

    // --- Models ---

    pub fn models(&self) -> Arc<Vec<MemoryModel>> {
        self.models.load_full()
    }

    pub fn upsert_model_in_memory(&self, model: MemoryModel) {
        let mut models = (*self.models.load_full()).clone();
        if let Some(existing) = models.iter_mut().find(|m| m.id == model.id) {
            *existing = model;
        } else {
            models.push(model);
        }
        self.models.store(Arc::new(models));
    }

    pub fn remove_model_from_memory(&self, model_id: i64) {
        let mut models = (*self.models.load_full()).clone();
        models.retain(|m| m.id != model_id);
        self.models.store(Arc::new(models));
    }

    // --- Model aliases ---

    pub fn model_aliases_snapshot(&self) -> Arc<HashMap<String, ModelAliasTarget>> {
        self.model_aliases.load_full()
    }

    pub fn upsert_model_alias_in_memory(&self, alias: String, target: ModelAliasTarget) {
        let mut aliases = (*self.model_aliases.load_full()).clone();
        aliases.insert(alias, target);
        self.model_aliases.store(Arc::new(aliases));
    }

    pub fn remove_model_alias_from_memory(&self, alias: &str) {
        let mut aliases = (*self.model_aliases.load_full()).clone();
        aliases.remove(alias);
        self.model_aliases.store(Arc::new(aliases));
    }

    // --- User permissions ---

    pub fn user_permissions_snapshot(&self) -> Arc<HashMap<i64, Vec<PermissionEntry>>> {
        self.user_permissions.load_full()
    }

    pub fn upsert_permission_in_memory(&self, user_id: i64, entry: PermissionEntry) {
        let mut perms = (*self.user_permissions.load_full()).clone();
        let entries = perms.entry(user_id).or_default();
        // Replace if same provider_id + model_pattern, else append
        if let Some(existing) = entries
            .iter_mut()
            .find(|e| e.provider_id == entry.provider_id && e.model_pattern == entry.model_pattern)
        {
            *existing = entry;
        } else {
            entries.push(entry);
        }
        self.user_permissions.store(Arc::new(perms));
    }

    pub fn remove_permission_from_memory(
        &self,
        user_id: i64,
        provider_id: Option<i64>,
        model_pattern: &str,
    ) {
        let mut perms = (*self.user_permissions.load_full()).clone();
        if let Some(entries) = perms.get_mut(&user_id) {
            entries.retain(|e| !(e.provider_id == provider_id && e.model_pattern == model_pattern));
            if entries.is_empty() {
                perms.remove(&user_id);
            }
        }
        self.user_permissions.store(Arc::new(perms));
    }

    // --- User rate limits ---

    pub fn user_rate_limits_snapshot(&self) -> Arc<HashMap<i64, Vec<RateLimitRule>>> {
        self.user_rate_limits.load_full()
    }

    pub fn upsert_rate_limit_in_memory(&self, user_id: i64, rule: RateLimitRule) {
        let mut limits = (*self.user_rate_limits.load_full()).clone();
        let rules = limits.entry(user_id).or_default();
        if let Some(existing) = rules
            .iter_mut()
            .find(|r| r.model_pattern == rule.model_pattern)
        {
            *existing = rule;
        } else {
            rules.push(rule);
        }
        self.user_rate_limits.store(Arc::new(limits));
    }

    pub fn remove_rate_limit_from_memory(&self, user_id: i64, model_pattern: &str) {
        let mut limits = (*self.user_rate_limits.load_full()).clone();
        if let Some(rules) = limits.get_mut(&user_id) {
            rules.retain(|r| r.model_pattern != model_pattern);
            if rules.is_empty() {
                limits.remove(&user_id);
            }
        }
        self.user_rate_limits.store(Arc::new(limits));
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
            engine: ArcSwap::from_pointee(self.engine.expect("GproxyEngine is required")),
            storage: Arc::new(ArcSwap::from_pointee(
                (*self.storage.expect("SeaOrmStorage is required")).clone(),
            )),
            storage_writes: self.storage_writes.expect("StorageWriteSender is required"),
            config: ArcSwap::from_pointee(self.config.unwrap_or_default()),
            users: ArcSwap::from_pointee(self.users),
            keys: ArcSwap::from_pointee(key_map),
            models: ArcSwap::from_pointee(Vec::new()),
            model_aliases: ArcSwap::from_pointee(HashMap::new()),
            provider_names: ArcSwap::from_pointee(HashMap::new()),
            user_permissions: ArcSwap::from_pointee(HashMap::new()),
            user_rate_limits: ArcSwap::from_pointee(HashMap::new()),
            user_quotas: DashMap::new(),
            rate_counters: RateLimitCounters::new(),
        }
    }
}

impl Default for AppStateBuilder {
    fn default() -> Self {
        Self::new()
    }
}
