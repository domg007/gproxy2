//! Shared application state handed to every HTTP handler.

use std::sync::Arc;

use crate::config::SharedConfig;
use crate::store::cache::CacheBackend;

/// Cheap-to-clone bundle of shared services (everything behind `Arc`).
/// Cloned per request by axum's state extractor.
#[derive(Clone)]
pub struct AppState {
    pub config: SharedConfig,
    pub cache: Arc<dyn CacheBackend>,
}

impl AppState {
    pub fn new(config: SharedConfig, cache: Arc<dyn CacheBackend>) -> Self {
        Self { config, cache }
    }
}
