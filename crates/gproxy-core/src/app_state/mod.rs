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

mod principals;
mod providers;
mod runtime;

#[cfg(test)]
use providers::pick_random_eligible_credential_from_snapshot;

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
