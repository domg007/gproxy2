use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;

use gproxy_sdk::provider::engine::GproxyEngine;
use gproxy_storage::{SeaOrmStorage, StorageWriteSender};

use crate::config::GlobalConfig;
use crate::middleware::model_alias::{self, ModelAliasMap};
use crate::middleware::permission::{self, PermissionMap};
use crate::middleware::rate_limit::{self, RateLimitConfigMap, RateLimitCounters, UserQuotaMap};
use crate::principal::{MemoryUser, MemoryUserKey};

// Re-export middleware types for convenience
pub use crate::middleware::model_alias::ModelAliasTarget;
pub use crate::middleware::permission::PermissionEntry;
pub use crate::middleware::rate_limit::{RateLimitRejection, RateLimitRule};

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
///
/// All fields use `ArcSwap` for lock-free reads and atomic hot-swapping.
/// HTTP clients live inside `GproxyEngine` — access them via `engine()`.
///
/// Model alias, permission, and rate limit logic lives in `gproxy-middleware`.
/// AppState holds the shared data references that middleware reads from.
pub struct AppState {
    engine: ArcSwap<GproxyEngine>,
    storage: Arc<ArcSwap<SeaOrmStorage>>,
    storage_writes: StorageWriteSender,
    config: ArcSwap<GlobalConfig>,
    users: ArcSwap<Vec<MemoryUser>>,
    keys: ArcSwap<HashMap<String, MemoryUserKey>>,

    // Model registry
    models: ArcSwap<Vec<MemoryModel>>,

    // Shared middleware data (middleware reads these directly)
    pub model_aliases: ModelAliasMap,
    pub permissions: PermissionMap,
    pub rate_limit_config: RateLimitConfigMap,
    pub user_quotas: UserQuotaMap,
    pub rate_counters: RateLimitCounters,
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
    pub fn authenticate_api_key(&self, api_key: &str) -> Option<MemoryUserKey> {
        let keys = self.keys.load();
        let key = keys.get(api_key)?;
        if !key.enabled {
            return None;
        }
        let users = self.users.load();
        let user_enabled = users.iter().any(|u| u.id == key.user_id && u.enabled);
        if !user_enabled {
            return None;
        }
        Some(key.clone())
    }

    /// Find model pricing info by model_id.
    pub fn find_model(&self, model_id: &str) -> Option<MemoryModel> {
        self.models
            .load()
            .iter()
            .find(|m| m.model_id == model_id && m.enabled)
            .cloned()
    }

    // Convenience wrappers that delegate to middleware functions:

    pub fn resolve_model_alias(&self, alias: &str) -> Option<ModelAliasTarget> {
        model_alias::resolve_alias(&self.model_aliases, alias)
    }

    pub fn check_model_permission(&self, user_id: i64, provider_id: i64, model: &str) -> bool {
        permission::check_permission(&self.permissions, user_id, provider_id, model)
    }

    pub fn check_rate_limit(&self, user_id: i64, model: &str) -> Result<(), RateLimitRejection> {
        rate_limit::check_rate_limit(
            &self.rate_limit_config,
            &self.user_quotas,
            &self.rate_counters,
            user_id,
            model,
        )
    }

    pub fn record_request(&self, user_id: i64, model: &str) {
        rate_limit::record_request(&self.rate_counters, user_id, model);
    }

    pub fn add_token_usage(&self, user_id: i64, tokens: i64, cost: f64) {
        rate_limit::add_token_usage(&self.user_quotas, user_id, tokens, cost);
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
            model_aliases: model_alias::new_model_alias_map(),
            permissions: permission::new_permission_map(),
            rate_limit_config: rate_limit::new_rate_limit_config_map(),
            user_quotas: rate_limit::new_user_quota_map(),
            rate_counters: RateLimitCounters::new(),
        }
    }
}

impl Default for AppStateBuilder {
    fn default() -> Self {
        Self::new()
    }
}
