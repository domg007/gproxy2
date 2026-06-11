//! Cross-instance config invalidation (§7.2, M8): subscribe to the Redis
//! invalidation channel; each message triggers a full snapshot rebuild + swap.
//! Native-only — redis + tokio::spawn are native; edge is single-instance.

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_invalidation_listener(state: crate::app::AppState) {
    use crate::store::cache::INVALIDATE_CHANNEL;
    tokio::spawn(async move {
        let reloader = state.clone();
        state
            .cache
            .subscribe(
                INVALIDATE_CHANNEL,
                Box::new(move |_payload| {
                    let st = reloader.clone();
                    tokio::spawn(async move {
                        if let Err(e) = st.reload_snapshot().await {
                            tracing::warn!(error = %e, "snapshot reload after invalidation failed");
                        }
                    });
                }),
            )
            .await;
        // subscribe returns when the connection drops; reconnection is a
        // documented M8 follow-up.
        tracing::warn!("invalidation listener stopped");
    });
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::Arc;

    use arc_swap::ArcSwap;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;

    use crate::app::AppState;
    use crate::app::snapshot::ControlPlaneSnapshot;
    use crate::config::{CacheConfig, PersistenceConfig, RuntimeConfig, UpstreamConfig};
    use crate::store::cache::{CacheBackend, MemoryCache};
    use crate::store::persistence::{FilePersistence, PersistenceBackend};

    /// Spawning the listener over a memory cache must not panic and must leave
    /// the process healthy. Memory `subscribe` is a no-op, so the listener task
    /// just returns; this confirms the wiring compiles and runs end to end.
    #[tokio::test]
    async fn spawn_listener_memory_is_noop_and_healthy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let persistence: Arc<dyn PersistenceBackend> = Arc::new(
            FilePersistence::open(dir.path().to_path_buf())
                .await
                .expect("file persistence"),
        );
        let config = Arc::new(RuntimeConfig {
            host: "127.0.0.1".into(),
            port: 0,
            cache: CacheConfig::Memory,
            persistence: PersistenceConfig::File {
                data_dir: dir.path().to_path_buf(),
            },
            upstream: UpstreamConfig::from_proxy_url(None),
            instance_id: 0,
            max_attempts: crate::config::DEFAULT_MAX_ATTEMPTS,
        });
        let cache: Arc<dyn CacheBackend> = Arc::new(MemoryCache::new());
        let snapshot = Arc::new(ArcSwap::from_pointee(ControlPlaneSnapshot::empty(1)));
        let channels = Arc::new(crate::channel::registry::ChannelRegistry::with_builtin());
        let cipher = crate::crypto::cipher_from_master_key(Some(&B64.encode([7u8; 32]))).unwrap();
        let state = AppState::new(
            config,
            cache,
            persistence,
            Arc::new(NoopUpstream),
            snapshot,
            channels,
            cipher,
        );

        super::spawn_invalidation_listener(state.clone());
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        // No panic; snapshot pointer still readable.
        assert_eq!(state.cp().version, 1);
    }

    struct NoopUpstream;
    #[async_trait::async_trait]
    impl crate::http::client::UpstreamClient for NoopUpstream {
        async fn send(
            &self,
            _req: http::Request<bytes::Bytes>,
        ) -> Result<http::Response<bytes::Bytes>, crate::http::client::ClientError> {
            Err(crate::http::client::ClientError::Transport("noop".into()))
        }
    }
}
