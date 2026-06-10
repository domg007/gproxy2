//! Lazily-built per-proxy upstream clients (§7.4). Fingerprint impersonation
//! extends the key in M7; until then distinct fingerprints share the proxy
//! client.

use std::sync::Arc;

use dashmap::DashMap;

use super::{UpstreamClient, WreqClient};

/// Per-proxy upstream client pool. The default client (already configured with
/// the global proxy, if any) serves `None`; distinct effective proxy URLs each
/// get one lazily-built [`WreqClient`].
pub struct ClientPool {
    default_client: Arc<dyn UpstreamClient>,
    by_proxy: DashMap<String, Arc<dyn UpstreamClient>>,
}

impl ClientPool {
    pub fn new(default_client: Arc<dyn UpstreamClient>) -> Self {
        Self {
            default_client,
            by_proxy: DashMap::new(),
        }
    }

    /// `None` → the default client; `Some(url)` → the cached client for that
    /// proxy, built on first use. A build failure (malformed proxy URL) warns
    /// and falls back to the default client rather than failing the attempt.
    pub fn for_proxy(&self, proxy: Option<&str>) -> Arc<dyn UpstreamClient> {
        let Some(url) = proxy else {
            return Arc::clone(&self.default_client);
        };
        if let Some(c) = self.by_proxy.get(url) {
            return Arc::clone(&c);
        }
        match WreqClient::with_proxy_url(Some(url)) {
            Ok(c) => {
                let c: Arc<dyn UpstreamClient> = Arc::new(c);
                self.by_proxy.insert(url.to_string(), Arc::clone(&c));
                c
            }
            Err(e) => {
                tracing::warn!(error = %e, "proxy client build failed; using default client");
                Arc::clone(&self.default_client)
            }
        }
    }
}
