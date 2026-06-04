//! Shared application state handed to every HTTP handler.

use std::sync::Arc;

use crate::config::RuntimeConfig;
use crate::http::client::UpstreamClient;
use crate::store::cache::CacheBackend;
use crate::store::persistence::PersistenceBackend;

/// Cheap-to-clone bundle of shared services (everything behind `Arc`).
/// Cloned per request by axum's state extractor.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RuntimeConfig>,
    pub cache: Arc<dyn CacheBackend>,
    pub persistence: Arc<dyn PersistenceBackend>,
    pub upstream: Arc<dyn UpstreamClient>,
}

impl AppState {
    pub fn new(
        config: Arc<RuntimeConfig>,
        cache: Arc<dyn CacheBackend>,
        persistence: Arc<dyn PersistenceBackend>,
        upstream: Arc<dyn UpstreamClient>,
    ) -> Self {
        Self {
            config,
            cache,
            persistence,
            upstream,
        }
    }
}
