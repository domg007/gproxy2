use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use gproxy_admin::{MemoryUser, MemoryUserKey};
use gproxy_provider::ChannelSettings;
use gproxy_provider::{
    ChannelCredentialStateStore, ChannelId, CredentialPickMode, CredentialRef, LocalTokenCount,
    LocalTokenizerError, LocalTokenizerStore, ProviderCredentialState, ProviderDefinition,
    ProviderDispatchTable, ProviderRegistry, UpstreamCredentialUpdate,
};
use gproxy_storage::{
    Scope, SeaOrmStorage, StorageWriteEvent, StorageWriteQueueError, StorageWriteSender,
    UserKeyQuery, UserKeyWrite, UserQuery, UserWrite,
};
use serde::{Deserialize, Serialize};
use wreq::Client as WreqClient;

use crate::http_clients::UpstreamHttpClients;
use crate::upstream_http::DEFAULT_SPOOF_EMULATION;

pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 8787;
pub const DEFAULT_DATA_DIR: &str = "./data";
pub const DEFAULT_DSN: &str = "sqlite://./data/gproxy.db?mode=rwc";
pub const DEFAULT_MASK_SENSITIVE_INFO: bool = true;
pub const DEFAULT_HF_URL: &str = "https://huggingface.co";
pub const DEFAULT_UPDATE_SOURCE: &str = "github";
pub const UPDATE_SOURCE_GITHUB: &str = "github";
pub const UPDATE_SOURCE_CLOUDFLARE: &str = "cloudflare";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalSettings {
    pub host: String,
    pub port: u16,
    pub proxy: Option<String>,
    pub spoof_emulation: String,
    pub update_source: String,
    pub hf_token: Option<String>,
    pub hf_url: Option<String>,
    pub admin_key: String,
    /// When true, redact sensitive fields in logs/events.
    /// Set false in debug sessions when full payload visibility is required.
    pub mask_sensitive_info: bool,
    pub dsn: String,
    pub data_dir: String,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            proxy: None,
            spoof_emulation: DEFAULT_SPOOF_EMULATION.to_string(),
            update_source: DEFAULT_UPDATE_SOURCE.to_string(),
            hf_token: None,
            hf_url: Some(DEFAULT_HF_URL.to_string()),
            admin_key: String::new(),
            mask_sensitive_info: DEFAULT_MASK_SENSITIVE_INFO,
            dsn: DEFAULT_DSN.to_string(),
            data_dir: DEFAULT_DATA_DIR.to_string(),
        }
    }
}

pub fn normalize_update_source(value: Option<&str>) -> String {
    let normalized = value
        .map(|item| item.trim().to_ascii_lowercase())
        .unwrap_or_else(|| DEFAULT_UPDATE_SOURCE.to_string());
    match normalized.as_str() {
        "cloudflare" | "cnb" | "web-hosted" | "s3" => UPDATE_SOURCE_CLOUDFLARE.to_string(),
        "github" => UPDATE_SOURCE_GITHUB.to_string(),
        _ => UPDATE_SOURCE_GITHUB.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeConfigSnapshot {
    pub global: GlobalSettings,
    pub providers: ProviderRegistry,
}

pub struct AppInfraState {
    storage: Arc<ArcSwap<SeaOrmStorage>>,
    storage_writes: StorageWriteSender,
    /// Standard/spoof upstream HTTP clients.
    ///
    /// The spoof client is dedicated to ClaudeCode OAuth/session cookie flow.
    http_clients: UpstreamHttpClients,
}

pub struct AppRuntimeState {
    config: Arc<ArcSwap<RuntimeConfigSnapshot>>,
    credential_states: Arc<ChannelCredentialStateStore>,
    tokenizers: Arc<LocalTokenizerStore>,
}

pub struct AppPrincipalState {
    users: Arc<ArcSwap<Vec<MemoryUser>>>,
    keys: Arc<ArcSwap<HashMap<String, MemoryUserKey>>>,
}

pub struct AppState {
    pub infra: AppInfraState,
    pub runtime: AppRuntimeState,
    pub principals: AppPrincipalState,
}

pub struct AppStateInit {
    pub storage: Arc<SeaOrmStorage>,
    pub storage_writes: StorageWriteSender,
    pub http: Arc<WreqClient>,
    pub spoof_http: Arc<WreqClient>,
    pub global: GlobalSettings,
    pub providers: ProviderRegistry,
    pub tokenizers: Arc<LocalTokenizerStore>,
    pub users: Vec<MemoryUser>,
    pub keys: HashMap<String, MemoryUserKey>,
}

impl AppState {
    pub fn new(init: AppStateInit) -> Self {
        let AppStateInit {
            storage,
            storage_writes,
            http,
            spoof_http,
            global,
            providers,
            tokenizers,
            users,
            keys,
        } = init;
        let snapshot = RuntimeConfigSnapshot { global, providers };
        let credential_states = Arc::new(ChannelCredentialStateStore::from_states(
            snapshot
                .providers
                .providers
                .iter()
                .flat_map(|provider| provider.credentials.channel_states.iter().cloned()),
        ));
        Self {
            infra: AppInfraState {
                storage: Arc::new(ArcSwap::from(storage)),
                storage_writes,
                http_clients: UpstreamHttpClients::new(http, spoof_http),
            },
            runtime: AppRuntimeState {
                config: Arc::new(ArcSwap::from_pointee(snapshot)),
                credential_states,
                tokenizers,
            },
            principals: AppPrincipalState {
                users: Arc::new(ArcSwap::from_pointee(users)),
                keys: Arc::new(ArcSwap::from_pointee(keys)),
            },
        }
    }

    pub fn load_config(&self) -> Arc<RuntimeConfigSnapshot> {
        self.runtime.config.load_full()
    }

    pub fn storage_writes(&self) -> &StorageWriteSender {
        &self.infra.storage_writes
    }

    pub fn credential_states(&self) -> &ChannelCredentialStateStore {
        self.runtime.credential_states.as_ref()
    }

    pub fn load_storage(&self) -> Arc<SeaOrmStorage> {
        self.infra.storage.load_full()
    }

    pub fn replace_storage(&self, storage: Arc<SeaOrmStorage>) {
        self.infra.storage.store(storage);
    }

    pub fn load_http(&self) -> Arc<WreqClient> {
        self.infra.http_clients.load_standard()
    }

    pub fn replace_http(&self, http: Arc<WreqClient>) {
        self.infra.http_clients.replace_standard(http);
    }

    pub fn load_spoof_http(&self) -> Arc<WreqClient> {
        self.infra.http_clients.load_spoof()
    }

    pub fn replace_spoof_http(&self, spoof_http: Arc<WreqClient>) {
        self.infra.http_clients.replace_spoof(spoof_http);
    }

    pub fn replace_http_clients(&self, http: Arc<WreqClient>, spoof_http: Arc<WreqClient>) {
        self.infra.http_clients.replace_all(http, spoof_http);
    }

    pub fn tokenizers(&self) -> Arc<LocalTokenizerStore> {
        self.runtime.tokenizers.clone()
    }

    pub fn upsert_tokenizer_vocab_in_memory(
        &self,
        model: impl Into<String>,
        tokenizer_json: Vec<u8>,
    ) -> Result<(), LocalTokenizerError> {
        self.runtime
            .tokenizers
            .upsert_memory_tokenizer_bytes(model, tokenizer_json)
    }

    pub async fn count_tokens_with_local_tokenizer(
        &self,
        model: &str,
        text: &str,
    ) -> Result<LocalTokenCount, LocalTokenizerError> {
        let http = self.load_http();
        let global = self.load_config().global.clone();
        self.runtime
            .tokenizers
            .count_text_tokens(
                http.as_ref(),
                global.hf_token.as_deref(),
                global.hf_url.as_deref(),
                model,
                text,
            )
            .await
    }

    pub async fn enqueue_storage_write(
        &self,
        event: StorageWriteEvent,
    ) -> Result<(), StorageWriteQueueError> {
        self.infra.storage_writes.enqueue(event).await
    }

    pub fn replace_config(&self, snapshot: RuntimeConfigSnapshot) {
        self.runtime.credential_states.replace_from_states(
            snapshot
                .providers
                .providers
                .iter()
                .flat_map(|provider| provider.credentials.channel_states.iter().cloned()),
        );
        self.runtime.config.store(Arc::new(snapshot));
    }

    pub fn load_users(&self) -> Arc<Vec<MemoryUser>> {
        self.principals.users.load_full()
    }

    pub fn replace_users(&self, users: Vec<MemoryUser>) {
        self.principals.users.store(Arc::new(users));
    }

    pub fn load_keys(&self) -> Arc<HashMap<String, MemoryUserKey>> {
        self.principals.keys.load_full()
    }

    pub fn replace_keys(&self, keys: HashMap<String, MemoryUserKey>) {
        self.principals.keys.store(Arc::new(keys));
    }

    pub fn query_users_in_memory(&self, query: &UserQuery) -> Vec<MemoryUser> {
        let mut rows: Vec<_> = self.principals.users.load().iter().cloned().collect();
        if let Scope::Eq(id) = query.id {
            rows.retain(|row| row.id == id);
        }
        if let Scope::Eq(name) = &query.name {
            rows.retain(|row| &row.name == name);
        }
        rows
    }

    pub fn query_user_keys_in_memory(&self, query: &UserKeyQuery) -> Vec<MemoryUserKey> {
        let mut rows: Vec<_> = self.principals.keys.load().values().cloned().collect();
        if let Scope::Eq(id) = query.id {
            rows.retain(|row| row.id == id);
        }
        if let Scope::Eq(user_id) = query.user_id {
            rows.retain(|row| row.user_id == user_id);
        }
        if let Scope::Eq(api_key) = &query.api_key {
            rows.retain(|row| &row.api_key == api_key);
        }
        rows
    }

    pub fn authenticate_api_key_in_memory(&self, api_key: &str) -> Option<MemoryUserKey> {
        self.principals.keys.load().get(api_key).cloned()
    }

    pub fn upsert_user_in_memory(&self, payload: UserWrite) {
        self.principals.users.rcu(|users| {
            let mut next = users.as_ref().clone();
            if let Some(existing) = next.iter_mut().find(|row| row.id == payload.id) {
                existing.name = payload.name.clone();
                existing.password = payload.password.clone();
                existing.enabled = payload.enabled;
            } else {
                next.push(MemoryUser {
                    id: payload.id,
                    name: payload.name.clone(),
                    password: payload.password.clone(),
                    enabled: payload.enabled,
                });
            }
            next.sort_by_key(|row| row.id);
            Arc::new(next)
        });
    }

    pub fn delete_user_in_memory(&self, id: i64) {
        self.principals.users.rcu(|users| {
            let mut next = users.as_ref().clone();
            next.retain(|row| row.id != id);
            Arc::new(next)
        });
        self.principals.keys.rcu(|keys| {
            let filtered = keys
                .iter()
                .filter(|(_, row)| row.user_id != id)
                .map(|(api_key, row)| (api_key.clone(), row.clone()))
                .collect::<HashMap<_, _>>();
            Arc::new(filtered)
        });
    }

    pub fn upsert_user_key_in_memory(&self, payload: UserKeyWrite) {
        self.principals.keys.rcu(|keys| {
            let mut next = keys.as_ref().clone();
            next.retain(|_, row| row.id != payload.id && row.api_key != payload.api_key);
            next.insert(
                payload.api_key.clone(),
                MemoryUserKey {
                    id: payload.id,
                    user_id: payload.user_id,
                    api_key: payload.api_key.clone(),
                    enabled: payload.enabled,
                },
            );
            Arc::new(next)
        });
    }

    pub fn delete_user_key_in_memory(&self, id: i64) {
        self.principals.keys.rcu(|keys| {
            let mut next = keys.as_ref().clone();
            next.retain(|_, row| row.id != id);
            Arc::new(next)
        });
    }

    pub fn upsert_credential_state(&self, state: gproxy_provider::ChannelCredentialState) {
        self.runtime.credential_states.upsert(state);
    }

    pub fn apply_upstream_credential_update_in_memory(
        &self,
        channel: &ChannelId,
        update: &UpstreamCredentialUpdate,
    ) -> bool {
        let mut snapshot = (*self.runtime.config.load_full()).clone();
        let applied = snapshot
            .providers
            .apply_upstream_credential_update(channel, update);
        if applied {
            self.runtime.config.store(Arc::new(snapshot));
        }
        applied
    }

    pub fn upsert_provider_in_memory(
        &self,
        channel: ChannelId,
        settings: ChannelSettings,
        dispatch: ProviderDispatchTable,
        credential_pick_mode: CredentialPickMode,
        enabled: bool,
    ) {
        let mut snapshot = (*self.runtime.config.load_full()).clone();
        if enabled {
            if let Some(existing) = snapshot.providers.get_mut(&channel) {
                existing.settings = settings;
                existing.dispatch = dispatch;
                existing.credential_pick_mode = credential_pick_mode;
            } else {
                snapshot.providers.upsert(ProviderDefinition {
                    channel: channel.clone(),
                    dispatch,
                    settings,
                    credential_pick_mode,
                    credentials: ProviderCredentialState::default(),
                });
            }
        } else {
            snapshot
                .providers
                .providers
                .retain(|item| item.channel != channel);
        }
        self.runtime.config.store(Arc::new(snapshot));
    }

    pub fn delete_provider_in_memory(&self, channel: &ChannelId) {
        let mut snapshot = (*self.runtime.config.load_full()).clone();
        snapshot
            .providers
            .providers
            .retain(|item| &item.channel != channel);
        self.runtime.config.store(Arc::new(snapshot));
    }

    pub fn upsert_provider_credential_in_memory(
        &self,
        channel: &ChannelId,
        credential: CredentialRef,
    ) -> bool {
        let mut snapshot = (*self.runtime.config.load_full()).clone();
        let applied = snapshot.providers.upsert_credential(channel, credential);
        if applied {
            self.runtime.config.store(Arc::new(snapshot));
        }
        applied
    }

    pub fn delete_provider_credential_in_memory(
        &self,
        channel: &ChannelId,
        credential_id: i64,
    ) -> bool {
        let mut snapshot = (*self.runtime.config.load_full()).clone();
        let Some(provider) = snapshot.providers.get_mut(channel) else {
            return false;
        };
        let removed = provider.delete_credential(credential_id).is_some();
        if removed {
            self.runtime
                .credential_states
                .remove(channel, credential_id);
            self.runtime.config.store(Arc::new(snapshot));
        }
        removed
    }

    pub fn get_provider_credential_in_memory(
        &self,
        channel: &ChannelId,
        credential_id: i64,
    ) -> Option<CredentialRef> {
        self.runtime
            .config
            .load()
            .providers
            .get(channel)
            .and_then(|provider| provider.credentials.credential(credential_id).cloned())
    }

    pub fn pick_random_eligible_credential(
        &self,
        channel: &ChannelId,
        model: Option<&str>,
        now_unix_ms: u64,
    ) -> Option<CredentialRef> {
        let config = self.runtime.config.load();
        pick_random_eligible_credential_from_snapshot(
            &config,
            &self.runtime.credential_states,
            channel,
            model,
            now_unix_ms,
        )
    }
}

fn pick_random_eligible_credential_from_snapshot(
    config: &RuntimeConfigSnapshot,
    states: &ChannelCredentialStateStore,
    channel: &ChannelId,
    model: Option<&str>,
    now_unix_ms: u64,
) -> Option<CredentialRef> {
    let provider = config.providers.get(channel)?;
    let credential = states.pick_random_eligible_credential(
        channel,
        provider.credentials.list_credentials(),
        model,
        now_unix_ms,
    )?;
    Some(credential.clone())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use gproxy_provider::ChannelSettings;
    use gproxy_provider::{
        ChannelCredential, ChannelCredentialState, ChannelId, CredentialHealth, CredentialPickMode,
        CredentialRef, CustomChannelCredential, LocalTokenizerStore, ProviderCredentialState,
        ProviderDefinition, ProviderDispatchTable, ProviderRegistry,
    };
    use gproxy_storage::{SeaOrmStorage, storage_write_channel};
    use tokio::runtime::Builder;
    use wreq::Client as WreqClient;

    use super::{
        AppState, AppStateInit, GlobalSettings, RuntimeConfigSnapshot, UPDATE_SOURCE_CLOUDFLARE,
        UPDATE_SOURCE_GITHUB, normalize_update_source,
        pick_random_eligible_credential_from_snapshot,
    };

    #[test]
    fn app_state_picks_eligible_credential() {
        let channel = ChannelId::parse("claude");
        let provider = ProviderDefinition {
            channel: channel.clone(),
            dispatch: ProviderDispatchTable::default(),
            settings: ChannelSettings::default(),
            credential_pick_mode: CredentialPickMode::RoundRobinWithCache,
            credentials: ProviderCredentialState {
                credentials: vec![CredentialRef {
                    id: 1,
                    label: None,
                    credential: ChannelCredential::Custom(CustomChannelCredential::default()),
                }],
                channel_states: vec![ChannelCredentialState {
                    channel: channel.clone(),
                    credential_id: 1,
                    health: CredentialHealth::Healthy,
                    checked_at_unix_ms: None,
                    last_error: None,
                }],
            },
        };
        let mut registry = ProviderRegistry::default();
        registry.upsert(provider);
        let snapshot = RuntimeConfigSnapshot {
            global: GlobalSettings::default(),
            providers: registry,
        };
        let states = gproxy_provider::ChannelCredentialStateStore::from_states(
            snapshot
                .providers
                .providers
                .iter()
                .flat_map(|provider| provider.credentials.channel_states.iter().cloned()),
        );
        let picked =
            pick_random_eligible_credential_from_snapshot(&snapshot, &states, &channel, None, 0);
        assert_eq!(picked.map(|item| item.id), Some(1));
    }

    #[test]
    fn normalize_update_source_maps_legacy_aliases() {
        assert_eq!(
            normalize_update_source(Some("cloudflare")),
            UPDATE_SOURCE_CLOUDFLARE
        );
        assert_eq!(
            normalize_update_source(Some("CNB")),
            UPDATE_SOURCE_CLOUDFLARE
        );
        assert_eq!(
            normalize_update_source(Some("web-hosted")),
            UPDATE_SOURCE_CLOUDFLARE
        );
        assert_eq!(
            normalize_update_source(Some("s3")),
            UPDATE_SOURCE_CLOUDFLARE
        );
        assert_eq!(
            normalize_update_source(Some("github")),
            UPDATE_SOURCE_GITHUB
        );
        assert_eq!(
            normalize_update_source(Some("unknown")),
            UPDATE_SOURCE_GITHUB
        );
        assert_eq!(normalize_update_source(None), UPDATE_SOURCE_GITHUB);
    }

    #[test]
    fn app_state_groups_infra_runtime_and_principals() {
        let (storage_writes, _rx) = storage_write_channel(4);
        let app_state = AppState::new(AppStateInit {
            storage: Arc::new(
                Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("runtime should build")
                    .block_on(SeaOrmStorage::connect("sqlite::memory:"))
                    .expect("memory storage should connect"),
            ),
            storage_writes,
            http: Arc::new(WreqClient::new()),
            spoof_http: Arc::new(WreqClient::new()),
            global: GlobalSettings::default(),
            providers: ProviderRegistry::default(),
            tokenizers: Arc::new(LocalTokenizerStore::new(std::path::PathBuf::from("/tmp"))),
            users: Vec::new(),
            keys: HashMap::new(),
        });

        let _ = app_state.load_config();
        let _ = app_state.storage_writes();
        let _ = app_state.credential_states();
    }
}
