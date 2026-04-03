use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;

use gproxy_sdk::provider::engine::GproxyEngine;
use gproxy_storage::{SeaOrmStorage, StorageWriteSender};

use crate::config::GlobalConfig;
use crate::middleware::model_alias::ModelAliasTarget;
use crate::middleware::permission::{self, PermissionEntry};
use crate::middleware::rate_limit::{RateLimitCounters, RateLimitRejection, RateLimitRule, find_matching_rule};
use crate::principal::{MemoryUser, MemoryUserKey};

// Re-export middleware types
pub use crate::middleware::model_alias::ModelAliasTarget as ModelAliasTargetExport;
pub use crate::middleware::permission::PermissionEntry as PermissionEntryExport;
pub use crate::middleware::rate_limit::{RateLimitRejection as RateLimitRejectionExport, RateLimitRule as RateLimitRuleExport};

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
    user_permissions: ArcSwap<HashMap<i64, Vec<PermissionEntry>>>,
    user_rate_limits: ArcSwap<HashMap<i64, Vec<RateLimitRule>>>,
    user_quotas: ArcSwap<HashMap<i64, (i64, f64)>>,
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

    pub fn find_model(&self, model_id: &str) -> Option<MemoryModel> {
        self.models
            .load()
            .iter()
            .find(|m| m.model_id == model_id && m.enabled)
            .cloned()
    }

    pub fn resolve_model_alias(&self, alias: &str) -> Option<ModelAliasTarget> {
        self.model_aliases.load().get(alias).cloned()
    }

    pub fn check_model_permission(&self, user_id: i64, provider_id: i64, model: &str) -> bool {
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
            && self.rate_counters.check_rpm(user_id, model) >= rpm as u32 {
                return Err(RateLimitRejection::Rpm { limit: rpm });
            }
        if let Some(rpd) = rule.rpd
            && self.rate_counters.check_rpd(user_id, model) >= rpd as u32 {
                return Err(RateLimitRejection::Rpd { limit: rpd });
            }
        if let Some(total_tokens) = rule.total_tokens {
            let used = self.user_quotas.load().get(&user_id).map(|(t, _)| *t).unwrap_or(0);
            if used >= total_tokens {
                return Err(RateLimitRejection::TokenQuota { used, limit: total_tokens });
            }
        }
        Ok(())
    }

    pub fn record_request(&self, user_id: i64, model: &str) {
        self.rate_counters.check_and_increment(user_id, model);
    }

    pub fn add_token_usage(&self, user_id: i64, tokens: i64, cost: f64) {
        let mut quotas = (*self.user_quotas.load_full()).clone();
        let entry = quotas.entry(user_id).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(tokens);
        entry.1 += cost;
        self.user_quotas.store(Arc::new(quotas));
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

    pub fn replace_models(&self, models: Vec<MemoryModel>) {
        self.models.store(Arc::new(models));
    }

    pub fn replace_model_aliases(&self, aliases: HashMap<String, ModelAliasTarget>) {
        self.model_aliases.store(Arc::new(aliases));
    }

    pub fn replace_user_permissions(&self, perms: HashMap<i64, Vec<PermissionEntry>>) {
        self.user_permissions.store(Arc::new(perms));
    }

    pub fn replace_user_rate_limits(&self, limits: HashMap<i64, Vec<RateLimitRule>>) {
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
                self.engine.expect("GproxyEngine is required"),
            ),
            storage: Arc::new(ArcSwap::from_pointee(
                (*self.storage.expect("SeaOrmStorage is required")).clone(),
            )),
            storage_writes: self.storage_writes.expect("StorageWriteSender is required"),
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
