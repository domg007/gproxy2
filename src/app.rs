//! Shared application state handed to every HTTP handler.

use std::sync::Arc;

use crate::config::SharedConfig;
use crate::store::cache::CacheBackend;
use crate::store::persistence::PersistenceBackend;

/// Cheap-to-clone bundle of shared services (everything behind `Arc`).
/// Cloned per request by axum's state extractor.
#[derive(Clone)]
pub struct AppState {
    pub config: SharedConfig,
    pub cache: Arc<dyn CacheBackend>,
    pub persistence: Arc<dyn PersistenceBackend>,
}

impl AppState {
    pub fn new(
        config: SharedConfig,
        cache: Arc<dyn CacheBackend>,
        persistence: Arc<dyn PersistenceBackend>,
    ) -> Self {
        Self {
            config,
            cache,
            persistence,
        }
    }
}
