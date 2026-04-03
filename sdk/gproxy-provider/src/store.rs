use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize};

use arc_swap::ArcSwap;
use serde::Serialize;
use serde_json::Value;

use crate::affinity::{CacheAffinityHint, CacheAffinityPool, DEFAULT_CACHE_AFFINITY_MAX_KEYS};
use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::dispatch::DispatchTable;
use crate::request::PreparedRequest;
use crate::response::{UpstreamError, UpstreamResponse};
use crate::retry::retry_with_credentials_max;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Serialize)]
pub struct ProviderSnapshot {
    pub name: String,
    pub settings: Value,
    pub credential_count: usize,
    pub credential_revision: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CredentialSnapshot {
    pub provider: String,
    pub index: usize,
    pub revision: u64,
    pub credential: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct CredentialUpdate {
    pub provider: String,
    pub index: usize,
    pub revision: u64,
    pub credential: Value,
}

pub(crate) struct ProviderExecuteResult {
    pub response: UpstreamResponse,
    pub credential_updates: Vec<CredentialUpdate>,
}

pub(crate) trait ProviderRuntime: Send + Sync {
    fn dispatch_table(&self) -> &DispatchTable;

    fn handle_local(
        &self,
        operation: &str,
        protocol: &str,
        body: &[u8],
    ) -> Option<Result<Vec<u8>, UpstreamError>>;

    fn finalize_request(&self, request: PreparedRequest) -> Result<PreparedRequest, UpstreamError>;

    fn normalize_response(&self, request: &PreparedRequest, body: Vec<u8>) -> Vec<u8>;

    fn execute<'a>(
        &'a self,
        request: PreparedRequest,
        affinity_hint: Option<CacheAffinityHint>,
        client: &'a wreq::Client,
    ) -> BoxFuture<'a, Result<ProviderExecuteResult, UpstreamError>>;

    fn snapshot(&self) -> Result<ProviderSnapshot, UpstreamError>;

    fn credential_snapshot(
        &self,
        index: usize,
    ) -> Result<Option<CredentialSnapshot>, UpstreamError>;

    fn credential_snapshots(&self) -> Result<Vec<CredentialSnapshot>, UpstreamError>;

    fn set_settings_json(&self, settings: Value) -> Result<(), UpstreamError>;

    fn add_credential_json(&self, credential: Value) -> Result<CredentialSnapshot, UpstreamError>;

    fn update_credential_json(
        &self,
        index: usize,
        credential: Value,
    ) -> Result<Option<CredentialSnapshot>, UpstreamError>;

    fn remove_credential_json(
        &self,
        index: usize,
    ) -> Result<Option<CredentialSnapshot>, UpstreamError>;

    fn apply_credential_update(&self, update: &CredentialUpdate) -> Result<bool, UpstreamError>;

    fn apply_credential_updates(
        &self,
        updates: &[CredentialUpdate],
    ) -> Result<Vec<bool>, UpstreamError>;
}

struct ProviderInstance<C: Channel> {
    name: String,
    channel: C,
    settings: ArcSwap<C::Settings>,
    credentials: ArcSwap<Vec<C::Credential>>,
    health: Mutex<Vec<C::Health>>,
    dispatch_table: DispatchTable,
    affinity_pool: CacheAffinityPool,
    round_robin_cursor: AtomicUsize,
    credential_revision: AtomicU64,
}

impl<C: Channel> ProviderInstance<C> {
    fn new(
        name: String,
        channel: C,
        settings: C::Settings,
        credentials: Vec<(C::Credential, C::Health)>,
    ) -> Self {
        let (credential_values, health_values): (Vec<_>, Vec<_>) = credentials.into_iter().unzip();
        Self {
            name,
            dispatch_table: channel.dispatch_table(),
            channel,
            settings: ArcSwap::from_pointee(settings),
            credentials: ArcSwap::from_pointee(credential_values),
            health: Mutex::new(health_values),
            affinity_pool: CacheAffinityPool::new(DEFAULT_CACHE_AFFINITY_MAX_KEYS),
            round_robin_cursor: AtomicUsize::new(0),
            credential_revision: AtomicU64::new(0),
        }
    }

    fn align_health_len(&self, target_len: usize) -> Vec<C::Health> {
        let mut guard = self.health.lock().unwrap();
        if guard.len() < target_len {
            guard.resize_with(target_len, Default::default);
        } else if guard.len() > target_len {
            guard.truncate(target_len);
        }
        guard.clone()
    }

    fn store_health_if_snapshot_unchanged(
        &self,
        credentials_snapshot: &Arc<Vec<C::Credential>>,
        healths: Vec<C::Health>,
        revision: u64,
    ) {
        let current_snapshot = self.credentials.load_full();
        let current_revision = self
            .credential_revision
            .load(std::sync::atomic::Ordering::SeqCst);
        if current_revision != revision || !Arc::ptr_eq(&current_snapshot, credentials_snapshot) {
            return;
        }

        let mut guard = self.health.lock().unwrap();
        *guard = healths;
    }

    fn credential_snapshot_from_value(
        &self,
        index: usize,
        revision: u64,
        credential: &C::Credential,
    ) -> Result<CredentialSnapshot, UpstreamError> {
        Ok(CredentialSnapshot {
            provider: self.name.clone(),
            index,
            revision,
            credential: serde_json::to_value(credential)
                .map_err(|e| UpstreamError::Channel(format!("serialize credential: {e}")))?,
        })
    }
}

impl<C: Channel> ProviderRuntime for ProviderInstance<C> {
    fn dispatch_table(&self) -> &DispatchTable {
        &self.dispatch_table
    }

    fn handle_local(
        &self,
        operation: &str,
        protocol: &str,
        body: &[u8],
    ) -> Option<Result<Vec<u8>, UpstreamError>> {
        self.channel.handle_local(operation, protocol, body)
    }

    fn finalize_request(&self, request: PreparedRequest) -> Result<PreparedRequest, UpstreamError> {
        let settings = self.settings.load();
        self.channel.finalize_request(&settings, request)
    }

    fn normalize_response(&self, request: &PreparedRequest, body: Vec<u8>) -> Vec<u8> {
        self.channel.normalize_response(request, body)
    }

    fn execute<'a>(
        &'a self,
        request: PreparedRequest,
        affinity_hint: Option<CacheAffinityHint>,
        client: &'a wreq::Client,
    ) -> BoxFuture<'a, Result<ProviderExecuteResult, UpstreamError>> {
        Box::pin(async move {
            let settings = self.settings.load_full();
            let credentials_snapshot = self.credentials.load_full();
            let revision = self
                .credential_revision
                .load(std::sync::atomic::Ordering::SeqCst);
            let health_snapshot = self.align_health_len(credentials_snapshot.len());
            let mut creds: Vec<(C::Credential, C::Health)> = credentials_snapshot
                .iter()
                .cloned()
                .zip(health_snapshot)
                .collect();

            let max_retries = settings.max_retries_on_429();
            let result = retry_with_credentials_max(
                &self.channel,
                &mut creds,
                &settings,
                &request,
                affinity_hint.as_ref(),
                &self.affinity_pool,
                &self.round_robin_cursor,
                max_retries,
                client,
                |req| crate::http_client::send_request(client, req),
            )
            .await;

            let updated_health: Vec<C::Health> =
                creds.iter().map(|(_, health)| health.clone()).collect();
            self.store_health_if_snapshot_unchanged(
                &credentials_snapshot,
                updated_health,
                revision,
            );

            let mut credential_updates = Vec::new();
            for (index, ((updated_credential, _), original_credential)) in
                creds.iter().zip(credentials_snapshot.iter()).enumerate()
            {
                let original_json = serde_json::to_value(original_credential)
                    .map_err(|e| UpstreamError::Channel(format!("serialize credential: {e}")))?;
                let updated_json = serde_json::to_value(updated_credential)
                    .map_err(|e| UpstreamError::Channel(format!("serialize credential: {e}")))?;
                if original_json != updated_json {
                    credential_updates.push(CredentialUpdate {
                        provider: self.name.clone(),
                        index,
                        revision,
                        credential: updated_json,
                    });
                }
            }

            result.map(|response| ProviderExecuteResult {
                response,
                credential_updates,
            })
        })
    }

    fn snapshot(&self) -> Result<ProviderSnapshot, UpstreamError> {
        let settings = self.settings.load();
        let credentials = self.credentials.load();
        let revision = self
            .credential_revision
            .load(std::sync::atomic::Ordering::SeqCst);
        Ok(ProviderSnapshot {
            name: self.name.clone(),
            settings: serde_json::to_value(&**settings)
                .map_err(|e| UpstreamError::Channel(format!("serialize settings: {e}")))?,
            credential_count: credentials.len(),
            credential_revision: revision,
        })
    }

    fn credential_snapshot(
        &self,
        index: usize,
    ) -> Result<Option<CredentialSnapshot>, UpstreamError> {
        let credentials = self.credentials.load();
        let revision = self
            .credential_revision
            .load(std::sync::atomic::Ordering::SeqCst);
        let Some(credential) = credentials.get(index) else {
            return Ok(None);
        };
        self.credential_snapshot_from_value(index, revision, credential)
            .map(Some)
    }

    fn credential_snapshots(&self) -> Result<Vec<CredentialSnapshot>, UpstreamError> {
        let credentials = self.credentials.load();
        let revision = self
            .credential_revision
            .load(std::sync::atomic::Ordering::SeqCst);
        credentials
            .iter()
            .enumerate()
            .map(|(index, credential)| {
                self.credential_snapshot_from_value(index, revision, credential)
            })
            .collect()
    }

    fn set_settings_json(&self, settings: Value) -> Result<(), UpstreamError> {
        let parsed: C::Settings = serde_json::from_value(settings)
            .map_err(|e| UpstreamError::Channel(format!("deserialize settings: {e}")))?;
        self.settings.store(Arc::new(parsed));
        Ok(())
    }

    fn add_credential_json(&self, credential: Value) -> Result<CredentialSnapshot, UpstreamError> {
        let parsed: C::Credential = serde_json::from_value(credential)
            .map_err(|e| UpstreamError::Channel(format!("deserialize credential: {e}")))?;
        let mut current = (*self.credentials.load_full()).clone();
        current.push(parsed);
        let index = current.len() - 1;
        let revision = self
            .credential_revision
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let snapshot = self.credential_snapshot_from_value(index, revision, &current[index])?;
        self.credentials.store(Arc::new(current));
        self.health.lock().unwrap().push(C::Health::default());
        Ok(snapshot)
    }

    fn update_credential_json(
        &self,
        index: usize,
        credential: Value,
    ) -> Result<Option<CredentialSnapshot>, UpstreamError> {
        let parsed: C::Credential = serde_json::from_value(credential)
            .map_err(|e| UpstreamError::Channel(format!("deserialize credential: {e}")))?;
        let mut current = (*self.credentials.load_full()).clone();
        let Some(slot) = current.get_mut(index) else {
            return Ok(None);
        };
        *slot = parsed;
        let revision = self
            .credential_revision
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let snapshot = self.credential_snapshot_from_value(index, revision, slot)?;
        self.credentials.store(Arc::new(current));
        Ok(Some(snapshot))
    }

    fn remove_credential_json(
        &self,
        index: usize,
    ) -> Result<Option<CredentialSnapshot>, UpstreamError> {
        let mut current = (*self.credentials.load_full()).clone();
        if index >= current.len() {
            return Ok(None);
        }
        let revision = self
            .credential_revision
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let removed = current.remove(index);
        self.credentials.store(Arc::new(current));
        let mut health = self.health.lock().unwrap();
        if index < health.len() {
            health.remove(index);
        }
        self.credential_snapshot_from_value(index, revision, &removed)
            .map(Some)
    }

    fn apply_credential_update(&self, update: &CredentialUpdate) -> Result<bool, UpstreamError> {
        self.apply_credential_updates(std::slice::from_ref(update))
            .map(|results| results.into_iter().next().unwrap_or(false))
    }

    fn apply_credential_updates(
        &self,
        updates: &[CredentialUpdate],
    ) -> Result<Vec<bool>, UpstreamError> {
        if updates.is_empty() {
            return Ok(Vec::new());
        }

        let current_revision = self
            .credential_revision
            .load(std::sync::atomic::Ordering::SeqCst);
        if updates
            .iter()
            .any(|update| update.revision != current_revision)
        {
            return Ok(vec![false; updates.len()]);
        }

        let mut current = (*self.credentials.load_full()).clone();
        let mut applied = vec![false; updates.len()];

        for (position, update) in updates.iter().enumerate() {
            let Some(credential) = current.get_mut(update.index) else {
                continue;
            };

            let mut patch_target = credential.clone();
            if !patch_target.apply_update(&update.credential) {
                patch_target = serde_json::from_value(update.credential.clone())
                    .map_err(|e| UpstreamError::Channel(format!("deserialize credential: {e}")))?;
            }
            *credential = patch_target;
            applied[position] = true;
        }

        self.credentials.store(Arc::new(current));
        self.credential_revision
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(applied)
    }
}

#[derive(Default)]
pub struct ProviderStoreBuilder {
    providers: HashMap<String, Arc<dyn ProviderRuntime>>,
}

impl ProviderStoreBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_provider<C: Channel>(
        mut self,
        name: impl Into<String>,
        channel: C,
        settings: C::Settings,
        credentials: Vec<(C::Credential, C::Health)>,
    ) -> Self {
        let name = name.into();
        let provider = Arc::new(ProviderInstance::new(
            name.clone(),
            channel,
            settings,
            credentials,
        ));
        self.providers.insert(name, provider);
        self
    }

    pub fn build(self) -> ProviderStore {
        ProviderStore {
            providers: ArcSwap::from(Arc::new(self.providers)),
        }
    }
}

pub struct ProviderStore {
    providers: ArcSwap<HashMap<String, Arc<dyn ProviderRuntime>>>,
}

impl ProviderStore {
    pub fn builder() -> ProviderStoreBuilder {
        ProviderStoreBuilder::new()
    }

    pub fn add_provider<C: Channel>(
        &self,
        name: impl Into<String>,
        channel: C,
        settings: C::Settings,
        credentials: Vec<(C::Credential, C::Health)>,
    ) {
        let name = name.into();
        let provider = Arc::new(ProviderInstance::new(
            name.clone(),
            channel,
            settings,
            credentials,
        ));
        let mut updated = (*self.providers.load_full()).clone();
        updated.insert(name, provider);
        self.providers.store(Arc::new(updated));
    }

    pub fn remove_provider(&self, name: &str) -> bool {
        let mut updated = (*self.providers.load_full()).clone();
        let removed = updated.remove(name).is_some();
        if removed {
            self.providers.store(Arc::new(updated));
        }
        removed
    }

    pub fn list_providers(&self) -> Result<Vec<ProviderSnapshot>, UpstreamError> {
        self.providers
            .load()
            .values()
            .map(|provider| provider.snapshot())
            .collect()
    }

    pub fn get_provider(&self, name: &str) -> Result<Option<ProviderSnapshot>, UpstreamError> {
        let Some(provider) = self.providers.load().get(name).cloned() else {
            return Ok(None);
        };
        provider.snapshot().map(Some)
    }

    pub fn list_credentials(
        &self,
        provider_name: Option<&str>,
    ) -> Result<Vec<CredentialSnapshot>, UpstreamError> {
        let providers = self.providers.load();
        let mut out = Vec::new();
        for (name, provider) in providers.iter() {
            if provider_name.is_some_and(|filter| filter != name) {
                continue;
            }
            out.extend(provider.credential_snapshots()?);
        }
        Ok(out)
    }

    pub fn get_credential(
        &self,
        provider_name: &str,
        index: usize,
    ) -> Result<Option<CredentialSnapshot>, UpstreamError> {
        let Some(provider) = self.providers.load().get(provider_name).cloned() else {
            return Ok(None);
        };
        provider.credential_snapshot(index)
    }

    pub fn update_provider_settings(
        &self,
        provider_name: &str,
        settings: Value,
    ) -> Result<bool, UpstreamError> {
        let Some(provider) = self.providers.load().get(provider_name).cloned() else {
            return Ok(false);
        };
        provider.set_settings_json(settings)?;
        Ok(true)
    }

    pub fn add_credential(
        &self,
        provider_name: &str,
        credential: Value,
    ) -> Result<Option<CredentialSnapshot>, UpstreamError> {
        let Some(provider) = self.providers.load().get(provider_name).cloned() else {
            return Ok(None);
        };
        provider.add_credential_json(credential).map(Some)
    }

    pub fn update_credential(
        &self,
        provider_name: &str,
        index: usize,
        credential: Value,
    ) -> Result<Option<CredentialSnapshot>, UpstreamError> {
        let Some(provider) = self.providers.load().get(provider_name).cloned() else {
            return Ok(None);
        };
        provider.update_credential_json(index, credential)
    }

    pub fn remove_credential(
        &self,
        provider_name: &str,
        index: usize,
    ) -> Result<Option<CredentialSnapshot>, UpstreamError> {
        let Some(provider) = self.providers.load().get(provider_name).cloned() else {
            return Ok(None);
        };
        provider.remove_credential_json(index)
    }

    pub fn apply_credential_update(
        &self,
        update: &CredentialUpdate,
    ) -> Result<bool, UpstreamError> {
        let Some(provider) = self.providers.load().get(&update.provider).cloned() else {
            return Ok(false);
        };
        provider.apply_credential_update(update)
    }

    pub fn apply_credential_updates(
        &self,
        updates: &[CredentialUpdate],
    ) -> Result<Vec<bool>, UpstreamError> {
        let mut grouped: HashMap<(String, u64), Vec<(usize, CredentialUpdate)>> = HashMap::new();
        for (index, update) in updates.iter().cloned().enumerate() {
            grouped
                .entry((update.provider.clone(), update.revision))
                .or_default()
                .push((index, update));
        }

        let mut results = vec![false; updates.len()];
        for ((provider_name, _revision), entries) in grouped {
            let Some(provider) = self.providers.load().get(&provider_name).cloned() else {
                continue;
            };
            let batch: Vec<CredentialUpdate> =
                entries.iter().map(|(_, update)| update.clone()).collect();
            let batch_results = provider.apply_credential_updates(&batch)?;
            for ((original_index, _), applied) in entries.into_iter().zip(batch_results.into_iter())
            {
                results[original_index] = applied;
            }
        }
        Ok(results)
    }

    pub(crate) fn get_runtime(&self, name: &str) -> Option<Arc<dyn ProviderRuntime>> {
        self.providers.load().get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{Channel, ChannelCredential, ChannelSettings};
    use crate::count_tokens::CountStrategy;
    use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
    use crate::health::SimpleHealth;
    use crate::response::ResponseClassification;
    use serde::{Deserialize, Serialize};

    #[derive(Clone)]
    struct TestChannel;

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct TestSettings {
        base_url: String,
    }

    impl ChannelSettings for TestSettings {
        fn base_url(&self) -> &str {
            &self.base_url
        }
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct TestCredential {
        token: String,
    }

    impl ChannelCredential for TestCredential {
        fn apply_update(&mut self, update: &serde_json::Value) -> bool {
            if let Some(token) = update.get("token").and_then(|v| v.as_str()) {
                self.token = token.to_string();
                true
            } else {
                false
            }
        }
    }

    impl Channel for TestChannel {
        const ID: &'static str = "test";
        type Settings = TestSettings;
        type Credential = TestCredential;
        type Health = SimpleHealth;

        fn dispatch_table(&self) -> DispatchTable {
            let mut table = DispatchTable::new();
            table.set(
                RouteKey::new("generate_content", "openai"),
                RouteImplementation::Passthrough,
            );
            table
        }

        fn prepare_request(
            &self,
            _credential: &Self::Credential,
            _settings: &Self::Settings,
            request: &PreparedRequest,
        ) -> Result<http::Request<Vec<u8>>, UpstreamError> {
            http::Request::builder()
                .method(request.method.clone())
                .uri("https://example.test")
                .body(request.body.clone())
                .map_err(|e| UpstreamError::RequestBuild(e.to_string()))
        }

        fn classify_response(
            &self,
            _status: u16,
            _headers: &http::HeaderMap,
            _body: &[u8],
        ) -> ResponseClassification {
            ResponseClassification::Success
        }

        fn count_strategy(&self) -> CountStrategy {
            CountStrategy::Local
        }
    }

    #[test]
    fn provider_store_query_and_mutation_apis_work() {
        let store = ProviderStore::builder()
            .add_provider(
                "test-provider",
                TestChannel,
                TestSettings {
                    base_url: "https://example.test".to_string(),
                },
                vec![(
                    TestCredential {
                        token: "alpha".to_string(),
                    },
                    SimpleHealth::default(),
                )],
            )
            .build();

        let provider = store
            .get_provider("test-provider")
            .expect("query should succeed")
            .expect("provider should exist");
        assert_eq!(provider.credential_count, 1);

        let credentials = store
            .list_credentials(Some("test-provider"))
            .expect("list credentials");
        assert_eq!(credentials.len(), 1);
        assert_eq!(credentials[0].credential["token"], "alpha");

        let added = store
            .add_credential("test-provider", serde_json::json!({ "token": "beta" }))
            .expect("add credential")
            .expect("provider should exist");
        assert_eq!(added.credential["token"], "beta");

        let updated = store
            .update_credential("test-provider", 0, serde_json::json!({ "token": "gamma" }))
            .expect("update credential")
            .expect("credential should exist");
        assert_eq!(updated.credential["token"], "gamma");

        let removed = store
            .remove_credential("test-provider", 1)
            .expect("remove credential")
            .expect("credential should exist");
        assert_eq!(removed.credential["token"], "beta");
    }

    #[test]
    fn provider_store_applies_multiple_credential_updates_from_same_revision() {
        let store = ProviderStore::builder()
            .add_provider(
                "test-provider",
                TestChannel,
                TestSettings {
                    base_url: "https://example.test".to_string(),
                },
                vec![
                    (
                        TestCredential {
                            token: "alpha".to_string(),
                        },
                        SimpleHealth::default(),
                    ),
                    (
                        TestCredential {
                            token: "beta".to_string(),
                        },
                        SimpleHealth::default(),
                    ),
                ],
            )
            .build();

        let credentials = store
            .list_credentials(Some("test-provider"))
            .expect("list credentials");
        let revision = credentials[0].revision;

        let applied = store
            .apply_credential_updates(&[
                CredentialUpdate {
                    provider: "test-provider".to_string(),
                    index: 0,
                    revision,
                    credential: serde_json::json!({ "token": "alpha-2" }),
                },
                CredentialUpdate {
                    provider: "test-provider".to_string(),
                    index: 1,
                    revision,
                    credential: serde_json::json!({ "token": "beta-2" }),
                },
            ])
            .expect("apply updates");
        assert_eq!(applied, vec![true, true]);

        let after = store
            .list_credentials(Some("test-provider"))
            .expect("list after updates");
        assert_eq!(after[0].credential["token"], "alpha-2");
        assert_eq!(after[1].credential["token"], "beta-2");
    }
}
