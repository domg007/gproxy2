use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;

use gproxy_sdk::provider::engine::GproxyEngine;
use gproxy_storage::{SeaOrmStorage, StorageWriteSender};

use crate::config::GlobalConfig;
use crate::principal::{MemoryUser, MemoryUserKey};

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
        }
    }
}

impl Default for AppStateBuilder {
    fn default() -> Self {
        Self::new()
    }
}
