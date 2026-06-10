//! Shared application state handed to every HTTP handler.

pub mod import;
pub mod models_index;
pub mod snapshot;

use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::channel::registry::ChannelRegistry;
use crate::config::RuntimeConfig;
use crate::http::client::UpstreamClient;
use crate::store::cache::CacheBackend;
use crate::store::persistence::PersistenceBackend;
use snapshot::ControlPlaneSnapshot;

/// Cheap-to-clone bundle of shared services (everything behind `Arc`).
/// Cloned per request by axum's state extractor.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RuntimeConfig>,
    pub cache: Arc<dyn CacheBackend>,
    pub persistence: Arc<dyn PersistenceBackend>,
    pub upstream: Arc<dyn UpstreamClient>,
    /// Sole control-plane snapshot (§7.2); replaced wholesale on invalidation.
    pub snapshot: Arc<ArcSwap<ControlPlaneSnapshot>>,
    /// Channel adapters keyed by id (§6.3).
    pub channels: Arc<ChannelRegistry>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<RuntimeConfig>,
        cache: Arc<dyn CacheBackend>,
        persistence: Arc<dyn PersistenceBackend>,
        upstream: Arc<dyn UpstreamClient>,
        snapshot: Arc<ArcSwap<ControlPlaneSnapshot>>,
        channels: Arc<ChannelRegistry>,
    ) -> Self {
        Self {
            config,
            cache,
            persistence,
            upstream,
            snapshot,
            channels,
        }
    }

    /// Load the current control-plane snapshot pointer (cheap).
    pub fn cp(&self) -> arc_swap::Guard<Arc<ControlPlaneSnapshot>> {
        self.snapshot.load()
    }

    /// Rebuild the snapshot from persistence and swap it in (next version).
    pub async fn reload_snapshot(&self) -> anyhow::Result<()> {
        let next = self.cp().version.wrapping_add(1);
        let snap = ControlPlaneSnapshot::build(self.persistence.as_ref(), next).await?;
        self.snapshot.store(Arc::new(snap));
        Ok(())
    }
}
