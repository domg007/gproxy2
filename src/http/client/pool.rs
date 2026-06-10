//! Lazily-built upstream clients keyed by `(proxy, fingerprint)` (§7.4). Each
//! distinct effective `(proxy, tls_fingerprint)` target gets one lazily-built
//! [`WreqClient`]; the default client serves the no-proxy/no-fingerprint case.

use std::sync::Arc;

use dashmap::DashMap;
use serde_json::Value;

use super::fingerprint::{fingerprint_hash, to_emulation};
use super::{UpstreamClient, WreqClient};

/// Upstream client pool keyed by `(proxy, fingerprint_hash)`. The default client
/// (already configured with the global proxy, if any) serves the `(None, None)`
/// target; every other distinct target gets one lazily-built [`WreqClient`].
pub struct ClientPool {
    default_client: Arc<dyn UpstreamClient>,
    /// Key: `(proxy URL or "", fingerprint hash or "")`.
    by_target: DashMap<(String, String), Arc<dyn UpstreamClient>>,
}

impl ClientPool {
    pub fn new(default_client: Arc<dyn UpstreamClient>) -> Self {
        Self {
            default_client,
            by_target: DashMap::new(),
        }
    }

    /// Resolve the client for an effective `(proxy, fingerprint)` target.
    ///
    /// `(None, None)` → the default client. Otherwise a client is built once per
    /// distinct target (proxy applied + header emulation from the fingerprint) and
    /// cached. A build failure (malformed proxy URL) warns and falls back to the
    /// default client rather than failing the attempt.
    pub fn for_target(
        &self,
        proxy: Option<&str>,
        fingerprint: Option<&Value>,
    ) -> Arc<dyn UpstreamClient> {
        let fp_hash = fingerprint.map(fingerprint_hash).unwrap_or_default();
        if proxy.is_none() && fp_hash.is_empty() {
            return Arc::clone(&self.default_client);
        }
        let key = (proxy.unwrap_or_default().to_string(), fp_hash);
        if let Some(c) = self.by_target.get(&key) {
            return Arc::clone(&c);
        }
        // Emulation is derived from the fingerprint; an unparsable fingerprint
        // yields None (proxy-only client) — see `fingerprint::to_emulation`.
        let emulation = fingerprint.and_then(to_emulation);
        match WreqClient::with_proxy_and_emulation(proxy, emulation) {
            Ok(c) => {
                let c: Arc<dyn UpstreamClient> = Arc::new(c);
                self.by_target.insert(key, Arc::clone(&c));
                c
            }
            Err(e) => {
                tracing::warn!(error = %e, "target client build failed; using default client");
                Arc::clone(&self.default_client)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::client::{ClientError, RespStream};
    use bytes::Bytes;
    use serde_json::json;

    /// Dummy default client; the pool never sends through it in these tests.
    struct Dummy;

    #[async_trait::async_trait]
    impl UpstreamClient for Dummy {
        async fn send(
            &self,
            _req: http::Request<Bytes>,
        ) -> Result<http::Response<Bytes>, ClientError> {
            unreachable!("test pool never sends")
        }
        async fn send_streaming(
            &self,
            _req: http::Request<Bytes>,
        ) -> Result<(http::StatusCode, http::HeaderMap, RespStream), ClientError> {
            unreachable!("test pool never sends")
        }
    }

    #[test]
    fn pool_keys_distinct_targets() {
        let pool = ClientPool::new(Arc::new(Dummy));
        let fp = json!({"headers": {"user-agent": "x"}});

        // A syntactically valid proxy URL that is never connected to (we only build).
        let a = pool.for_target(Some("http://127.0.0.1:9"), None);
        let b = pool.for_target(Some("http://127.0.0.1:9"), Some(&fp));
        // Distinct targets (fingerprint differs) → distinct client instances.
        assert!(!Arc::ptr_eq(&a, &b));

        // Same key repeated → same cached client instance.
        let a2 = pool.for_target(Some("http://127.0.0.1:9"), None);
        assert!(Arc::ptr_eq(&a, &a2));
        let b2 = pool.for_target(Some("http://127.0.0.1:9"), Some(&fp));
        assert!(Arc::ptr_eq(&b, &b2));

        // (None, None) → the default client (distinct from the built ones).
        let def = pool.for_target(None, None);
        assert!(!Arc::ptr_eq(&def, &a));
        assert!(Arc::ptr_eq(&def, &pool.for_target(None, None)));
    }
}
