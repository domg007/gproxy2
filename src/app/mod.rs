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
    /// Envelope cipher for stored secrets (§14.1): seals at import, opens at
    /// use. Keyless boots get a [`crate::crypto::NoopCipher`] (plaintext).
    pub cipher: Arc<dyn crate::crypto::SecretCipher>,
    /// Per-instance passive health: breakers, credential cooldowns, latency
    /// EWMA (§3.2). Soft state — restart clears.
    pub health: Arc<crate::health::HealthState>,
    /// Per-proxy upstream client pool (§7.4): failover resolves the effective
    /// proxy per attempt and picks a client here. `upstream` stays the default
    /// client (also used by tokenizer downloads). wasm/non-wreq builds have no
    /// pool — failover uses `upstream` directly.
    #[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
    pub client_pool: Arc<crate::http::client::ClientPool>,
    /// Global tokenizer registry (§6.3), backed by the shared persistence
    /// backend for downloaded vocabs. `main.rs` only flips download
    /// enablement from instance settings before serving.
    #[cfg(feature = "count-local")]
    pub tokenizers: Arc<crate::tokenize::TokenizerRegistry>,
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
        cipher: Arc<dyn crate::crypto::SecretCipher>,
    ) -> Self {
        #[cfg(feature = "count-local")]
        let tokenizers = Arc::new(crate::tokenize::TokenizerRegistry::new(
            Arc::clone(&persistence),
            Arc::clone(&upstream),
        ));
        #[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
        let client_pool = Arc::new(crate::http::client::ClientPool::new(Arc::clone(&upstream)));
        Self {
            config,
            cache,
            persistence,
            upstream,
            snapshot,
            channels,
            cipher,
            health: Arc::new(crate::health::HealthState::new()),
            #[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
            client_pool,
            #[cfg(feature = "count-local")]
            tokenizers,
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
