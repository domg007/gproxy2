//! Single-flight OAuth credential refresh (§14.5). Lazy: only when the channel
//! says the decrypted secret needs it. Per-credential local mutex serialises
//! concurrent refreshes (many providers rotate refresh_token each call, so a
//! double refresh kills the credential). Multi-instance distributed lock
//! (redis, key = cred id) is an M8 seam — local mutex only for now.
//!
//! The mutex is `futures_util::lock::Mutex` (runtime-agnostic): tokio is a
//! native-only dependency, so the edge/wasm build cannot use `tokio::sync`.

use std::sync::Arc;

use dashmap::DashMap;
use futures_util::lock::Mutex;
use serde_json::Value;

use crate::app::AppState;
use crate::channel::{Channel, ChannelError};
use crate::store::persistence::records::{Credential, CredentialInput};

/// Serialises refreshes per credential id so concurrent requests cannot rotate
/// the same credential twice.
pub struct RefreshOrchestrator {
    locks: DashMap<i64, Arc<Mutex<()>>>,
}

impl Default for RefreshOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl RefreshOrchestrator {
    pub fn new() -> Self {
        Self {
            locks: DashMap::new(),
        }
    }

    /// Return a fresh decrypted secret for the credential, refreshing if the
    /// channel deems it stale. `opened` is the already-decrypted current secret.
    /// `force` skips the staleness gate (AuthDead-triggered forced refresh).
    pub async fn ensure_fresh(
        &self,
        state: &AppState,
        channel: &Arc<dyn Channel>,
        credential: &Credential,
        opened: Value,
        force: bool,
    ) -> Result<Value, ChannelError> {
        if !force && !channel.needs_refresh(&opened) {
            return Ok(opened);
        }
        let lock = self
            .locks
            .entry(credential.id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;
        // Loser re-check (single-flight): re-read the credential + re-open. Two
        // discriminators, because `force` and the lazy path differ:
        //   * lazy (force=false): a peer that rotated leaves the secret no longer
        //     stale — `needs_refresh(&current)` is false → use it, no 2nd refresh.
        //   * forced (force=true): the token may still LOOK fresh (the AuthDead is
        //     clock-skew / server-side revocation), so `needs_refresh` can't tell
        //     winner from loser. Instead compare against what THIS caller opened:
        //     if the re-read secret CHANGED, a concurrent forced refresh already
        //     rotated it → use that (a 2nd rotation would double-spend a single-use
        //     refresh_token and kill the cred). If unchanged, this caller is the
        //     winner and must honor `force`.
        let current = reread_open(state, credential)
            .await
            .unwrap_or_else(|| opened.clone());
        if !force && !channel.needs_refresh(&current) {
            return Ok(current);
        }
        if force && current != opened {
            return Ok(current);
        }
        // Refresh goes through the default client (no proxy pool needed).
        let client = Arc::clone(&state.upstream);
        let fresh = channel.refresh(&client, &current).await?;
        // seal + writeback + publish — channel error already propagated above so
        // the caller cools + skips the credential on a failed refresh.
        let sealed = state
            .cipher
            .seal(&fresh)
            .map_err(|e| ChannelError::Build(format!("seal refreshed secret: {e}")))?;
        writeback(state, credential, sealed)
            .await
            .map_err(|e| ChannelError::Build(format!("persist refreshed secret: {e}")))?;
        state
            .cache
            .publish("cred", credential.id.to_string().as_bytes())
            .await;
        Ok(fresh)
    }
}

/// Re-read the credential from persistence and decrypt its secret. Returns
/// `None` if the credential was deleted mid-refresh (caller falls back to the
/// secret it already holds) or if the re-read/open fails.
async fn reread_open(state: &AppState, credential: &Credential) -> Option<Value> {
    let creds = state
        .persistence
        .list_credentials(credential.provider_id)
        .await
        .ok()?;
    let stored = creds.into_iter().find(|c| c.id == credential.id)?;
    state.cipher.open(&stored.secret_json).ok()
}

/// Persist the re-sealed secret, copying every other field from the current
/// credential record (id = Some → update in place).
async fn writeback(state: &AppState, credential: &Credential, sealed: Value) -> anyhow::Result<()> {
    let input = CredentialInput {
        id: Some(credential.id),
        provider_id: credential.provider_id,
        name: credential.name.clone(),
        kind: credential.kind.clone(),
        secret_json: sealed,
        weight: credential.weight,
        rpm_limit: credential.rpm_limit,
        tpm_limit: credential.tpm_limit,
        proxy_url: credential.proxy_url.clone(),
        tls_fingerprint: credential.tls_fingerprint.clone(),
        enabled: credential.enabled,
    };
    state.persistence.upsert_credential(input).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use arc_swap::ArcSwap;
    use async_trait::async_trait;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as B64;
    use bytes::Bytes;
    use http::StatusCode;
    use serde_json::json;

    use crate::app::snapshot::ControlPlaneSnapshot;
    use crate::channel::{Disposition, PrepareCtx, PreparedRequest, TransportKind};
    use crate::config::{CacheConfig, PersistenceConfig, RuntimeConfig, UpstreamConfig};
    use crate::crypto::envelope::is_envelope;
    use crate::http::client::{ClientError, UpstreamClient};
    use crate::protocol::ContentGenerationKind;
    use crate::store::persistence::FilePersistence;
    use crate::store::persistence::records::CredentialInput;

    /// Channel whose refresh emits `{"access_token":"new"}` and is "stale" until
    /// the secret carries that marker — so a loser's re-check short-circuits.
    struct FakeRefreshChannel {
        refreshes: Arc<AtomicUsize>,
        sleep_ms: u64,
    }

    #[async_trait]
    impl Channel for FakeRefreshChannel {
        fn id(&self) -> &'static str {
            "fake_refresh"
        }
        fn target_kind(&self) -> ContentGenerationKind {
            ContentGenerationKind::OpenAiChatCompletions
        }
        fn prepare(&self, _ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
            Err(ChannelError::Unsupported("prepare"))
        }
        fn classify(
            &self,
            status: StatusCode,
            headers: &http::HeaderMap,
            _body: &Bytes,
        ) -> Disposition {
            Disposition::from_http(status, headers)
        }
        fn transport(&self) -> TransportKind {
            TransportKind::Http
        }
        fn needs_refresh(&self, secret: &Value) -> bool {
            secret.get("access_token").and_then(Value::as_str) != Some("new")
        }
        async fn refresh(
            &self,
            _client: &Arc<dyn UpstreamClient>,
            _secret: &Value,
        ) -> Result<Value, ChannelError> {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            if self.sleep_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.sleep_ms)).await;
            }
            Ok(json!({"access_token": "new"}))
        }
    }

    struct NoopUpstream;
    #[async_trait]
    impl UpstreamClient for NoopUpstream {
        async fn send(
            &self,
            _req: http::Request<Bytes>,
        ) -> Result<http::Response<Bytes>, ClientError> {
            Err(ClientError::Transport("noop".into()))
        }
    }

    fn cipher() -> Arc<dyn crate::crypto::SecretCipher> {
        crate::crypto::cipher_from_master_key(Some(&B64.encode([9u8; 32]))).unwrap()
    }

    /// AppState over a FilePersistence tempdir + MemoryCache + EnvelopeCipher,
    /// seeded with one credential whose secret is `seed` (sealed).
    async fn state_with_cred(
        cipher: Arc<dyn crate::crypto::SecretCipher>,
        seed: Value,
    ) -> (AppState, Credential, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let persistence: Arc<dyn crate::store::persistence::PersistenceBackend> = Arc::new(
            FilePersistence::open(dir.path().to_path_buf())
                .await
                .expect("file persistence"),
        );
        let sealed = cipher.seal(&seed).unwrap();
        let credential = persistence
            .upsert_credential(CredentialInput {
                id: None,
                provider_id: 1,
                name: Some("c".into()),
                kind: "oauth".into(),
                secret_json: sealed,
                weight: 100,
                rpm_limit: None,
                tpm_limit: None,
                proxy_url: None,
                tls_fingerprint: None,
                enabled: true,
            })
            .await
            .expect("seed credential");
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
        let cache: Arc<dyn crate::store::cache::CacheBackend> =
            Arc::new(crate::store::cache::MemoryCache::new());
        let upstream: Arc<dyn UpstreamClient> = Arc::new(NoopUpstream);
        let snapshot = Arc::new(ArcSwap::from_pointee(ControlPlaneSnapshot::empty(1)));
        let channels = Arc::new(crate::channel::registry::ChannelRegistry::with_builtin());
        let state = AppState::new(
            config,
            cache,
            persistence,
            upstream,
            snapshot,
            channels,
            cipher,
        );
        (state, credential, dir)
    }

    /// Read the sealed secret currently stored for `cred`.
    async fn stored_secret(state: &AppState, cred: &Credential) -> Value {
        state
            .persistence
            .list_credentials(cred.provider_id)
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.id == cred.id)
            .unwrap()
            .secret_json
    }

    #[tokio::test]
    async fn refreshes_and_writes_back_sealed() {
        let cipher = cipher();
        let (state, cred, _dir) =
            state_with_cred(cipher.clone(), json!({"access_token": "old"})).await;
        let refreshes = Arc::new(AtomicUsize::new(0));
        let channel: Arc<dyn Channel> = Arc::new(FakeRefreshChannel {
            refreshes: refreshes.clone(),
            sleep_ms: 0,
        });

        let got = state
            .refresh
            .ensure_fresh(
                &state,
                &channel,
                &cred,
                json!({"access_token": "old"}),
                false,
            )
            .await
            .unwrap();

        assert_eq!(got, json!({"access_token": "new"}));
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        // Persisted secret is a real envelope that opens to the refreshed value.
        let stored = stored_secret(&state, &cred).await;
        assert!(is_envelope(&stored), "stored secret should be sealed");
        assert_eq!(
            cipher.open(&stored).unwrap(),
            json!({"access_token": "new"})
        );
    }

    #[tokio::test]
    async fn no_refresh_when_fresh() {
        let cipher = cipher();
        let fresh = json!({"access_token": "new"});
        let (state, cred, _dir) = state_with_cred(cipher.clone(), fresh.clone()).await;
        let before = stored_secret(&state, &cred).await;
        let refreshes = Arc::new(AtomicUsize::new(0));
        let channel: Arc<dyn Channel> = Arc::new(FakeRefreshChannel {
            refreshes: refreshes.clone(),
            sleep_ms: 0,
        });

        let got = state
            .refresh
            .ensure_fresh(&state, &channel, &cred, fresh.clone(), false)
            .await
            .unwrap();

        assert_eq!(got, fresh);
        assert_eq!(refreshes.load(Ordering::SeqCst), 0, "refresh must not run");
        // Persistence untouched.
        assert_eq!(stored_secret(&state, &cred).await, before);
    }

    #[tokio::test]
    async fn single_flight_refreshes_once() {
        let cipher = cipher();
        let (state, cred, _dir) =
            state_with_cred(cipher.clone(), json!({"access_token": "old"})).await;
        let refreshes = Arc::new(AtomicUsize::new(0));
        let channel: Arc<dyn Channel> = Arc::new(FakeRefreshChannel {
            refreshes: refreshes.clone(),
            sleep_ms: 20,
        });

        let stale = json!({"access_token": "old"});
        let (a, b) = tokio::join!(
            state
                .refresh
                .ensure_fresh(&state, &channel, &cred, stale.clone(), false),
            state
                .refresh
                .ensure_fresh(&state, &channel, &cred, stale.clone(), false),
        );

        assert_eq!(a.unwrap(), json!({"access_token": "new"}));
        assert_eq!(b.unwrap(), json!({"access_token": "new"}));
        // Loser re-reads the winner's sealed result and short-circuits.
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    }

    /// Channel that ALWAYS reports fresh (`needs_refresh == false`) yet whose
    /// `refresh` rotates the token — models the forced-refresh case where the
    /// staleness view can't distinguish winner from loser, so the loser must
    /// fall back on "the secret changed under the lock".
    struct AlwaysFreshRotatingChannel {
        refreshes: Arc<AtomicUsize>,
        sleep_ms: u64,
    }

    #[async_trait]
    impl Channel for AlwaysFreshRotatingChannel {
        fn id(&self) -> &'static str {
            "always_fresh"
        }
        fn target_kind(&self) -> ContentGenerationKind {
            ContentGenerationKind::OpenAiChatCompletions
        }
        fn prepare(&self, _ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
            Err(ChannelError::Unsupported("prepare"))
        }
        fn classify(
            &self,
            status: StatusCode,
            headers: &http::HeaderMap,
            _body: &Bytes,
        ) -> Disposition {
            Disposition::from_http(status, headers)
        }
        fn transport(&self) -> TransportKind {
            TransportKind::Http
        }
        fn needs_refresh(&self, _secret: &Value) -> bool {
            false
        }
        async fn refresh(
            &self,
            _client: &Arc<dyn UpstreamClient>,
            _secret: &Value,
        ) -> Result<Value, ChannelError> {
            let n = self.refreshes.fetch_add(1, Ordering::SeqCst) + 1;
            if self.sleep_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.sleep_ms)).await;
            }
            Ok(json!({ "access_token": format!("rotated-{n}") }))
        }
    }

    /// Two concurrent FORCED refreshes (AuthDead on both) of the same credential
    /// must rotate the token exactly once. A single-use refresh_token rotated
    /// twice would be killed upstream; the loser sees the secret changed under
    /// the lock and reuses the winner's token instead of refreshing again.
    #[tokio::test]
    async fn forced_single_flight_rotates_once() {
        let cipher = cipher();
        let (state, cred, _dir) =
            state_with_cred(cipher.clone(), json!({"access_token": "orig"})).await;
        let refreshes = Arc::new(AtomicUsize::new(0));
        let channel: Arc<dyn Channel> = Arc::new(AlwaysFreshRotatingChannel {
            refreshes: refreshes.clone(),
            sleep_ms: 20,
        });

        let orig = json!({"access_token": "orig"});
        let (a, b) = tokio::join!(
            state
                .refresh
                .ensure_fresh(&state, &channel, &cred, orig.clone(), true),
            state
                .refresh
                .ensure_fresh(&state, &channel, &cred, orig.clone(), true),
        );

        // Exactly one rotation; both callers see the same rotated token.
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(a.unwrap(), b.unwrap());
    }
}
