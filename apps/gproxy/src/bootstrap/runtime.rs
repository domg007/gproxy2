use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use gproxy_admin::{MemoryUser, MemoryUserKey};
use gproxy_core::{AppState, AppStateInit, GlobalSettings};
use gproxy_provider::channels::{aistudio, claude, deepseek, nvidia, openai, vertexexpress};
use gproxy_provider::{
    BUILTIN_CHANNELS, BuiltinChannel, ChannelCredential, ChannelCredentialState, ChannelId,
    CredentialHealth, CredentialRef, CustomChannelCredential, LocalTokenizerStore, ModelCooldown,
    ProviderCredentialState, ProviderDefinition, ProviderDispatchTable, ProviderRegistry,
    parse_provider_settings_value_for_channel, provider_settings_to_json_string,
};
use gproxy_storage::{
    CredentialQuery, CredentialStatusQuery, CredentialStatusWrite, CredentialWrite,
    GlobalSettingsWrite, ProviderQuery, ProviderWrite, Scope, SeaOrmStorage, StorageWriteBatch,
    StorageWriteEvent, StorageWriteSink, StorageWriteSinkError, StorageWriteWorkerConfig,
    UserKeyQuery, UserKeyWrite, UserQuery, UserWrite, spawn_storage_write_worker,
    storage_write_channel,
};
use rand::RngExt as _;
use wreq::Client as WreqClient;
use wreq::Proxy;
use wreq::header::{ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, HeaderMap, HeaderValue};
use wreq_util::Emulation;

use crate::bootstrap::cli::CliArgs;
use crate::bootstrap::config::{
    BootstrapConfig, CredentialConfigFile, CredentialHealthConfigFile, DEFAULT_CONFIG_PATH,
};

pub struct Bootstrap {
    pub config_path: std::path::PathBuf,
    pub config: BootstrapConfig,
    pub storage: Arc<SeaOrmStorage>,
    pub state: Arc<AppState>,
    pub storage_write_worker: tokio::task::JoinHandle<Result<(), StorageWriteSinkError>>,
}

const CLIENT_CONNECT_TIMEOUT_SECS: u64 = 5;
const CLIENT_REQUEST_TIMEOUT_SECS: u64 = 86400;
const CLIENT_STREAM_IDLE_TIMEOUT_SECS: u64 = 30;

pub async fn bootstrap_from_env() -> Result<Bootstrap> {
    let args = CliArgs::parse();
    bootstrap(args).await
}

pub async fn bootstrap(args: CliArgs) -> Result<Bootstrap> {
    let config_path = args.config.clone();
    let use_in_memory_defaults =
        !config_path.exists() && config_path == std::path::Path::new(DEFAULT_CONFIG_PATH);
    let mut config = BootstrapConfig::load(&config_path)?;
    apply_cli_env_overrides(&mut config, &args);
    if use_in_memory_defaults {
        eprintln!(
            "bootstrap: {} not found, using in-memory defaults",
            DEFAULT_CONFIG_PATH
        );
    }
    let mut global = merge_global_settings(&config);
    if global.data_dir.trim().is_empty() {
        return Err(anyhow::anyhow!("global.data_dir cannot be empty"));
    }
    if global.dsn.trim().is_empty() {
        return Err(anyhow::anyhow!("global.dsn cannot be empty"));
    }

    std::fs::create_dir_all(&global.data_dir)
        .with_context(|| format!("create data dir {}", global.data_dir))?;
    let tokenizer_cache_dir = std::path::Path::new(&global.data_dir).join("tokenizers");
    std::fs::create_dir_all(&tokenizer_cache_dir).with_context(|| {
        format!(
            "create tokenizer cache dir {}",
            tokenizer_cache_dir.to_string_lossy()
        )
    })?;

    let storage = Arc::new(
        SeaOrmStorage::connect(&global.dsn)
            .await
            .with_context(|| format!("connect storage dsn={}", global.dsn))?,
    );
    storage.sync().await.context("sync storage schema")?;

    let (write_tx, write_rx) = storage_write_channel(config.runtime.storage_write_queue_capacity);
    let write_worker = spawn_storage_write_worker(
        storage.clone(),
        write_rx,
        StorageWriteWorkerConfig {
            max_batch_size: config.runtime.storage_write_max_batch_size,
            aggregate_window: Duration::from_millis(
                config.runtime.storage_write_aggregate_window_ms,
            ),
        },
    );

    let mut registry = build_provider_registry(&config);
    let provider_ids = seed_registry_providers(&storage, &registry)
        .await
        .context("seed provider registry into storage")?;
    seed_registry_credentials_and_statuses(&storage, &mut registry, &provider_ids)
        .await
        .context("seed registry credentials and statuses into storage")?;
    let mut users = storage
        .list_users(&UserQuery {
            id: Scope::All,
            name: Scope::All,
        })
        .await
        .context("load users into memory")?
        .into_iter()
        .map(|row| MemoryUser {
            id: row.id,
            name: row.name,
            enabled: row.enabled,
        })
        .collect::<Vec<_>>();
    let keys_rows = storage
        .list_user_keys_for_memory(&UserKeyQuery {
            id: Scope::All,
            user_id: Scope::All,
            api_key: Scope::All,
        })
        .await
        .context("load user_keys into memory")?;
    let mut keys: std::collections::HashMap<String, MemoryUserKey> = keys_rows
        .into_iter()
        .map(|row| {
            (
                row.api_key.clone(),
                MemoryUserKey {
                    id: row.id,
                    user_id: row.user_id,
                    api_key: row.api_key,
                    enabled: row.enabled,
                },
            )
        })
        .collect();
    let (admin_user_write, admin_key_write) =
        ensure_admin_principal(&mut global, &mut users, &mut keys)?;
    seed_admin_principal(&storage, admin_user_write, admin_key_write)
        .await
        .context("seed admin principal into storage")?;
    seed_global_settings(&storage, &global)
        .await
        .context("seed global settings into storage")?;

    let http_client = build_http_client(global.proxy.as_deref())
        .context("build standard upstream http client")?;
    let spoof_http_client = build_claudecode_spoof_client(global.proxy.as_deref())
        .context("build claudecode spoof http client")?;
    let tokenizer_store = Arc::new(LocalTokenizerStore::new(tokenizer_cache_dir));
    if let Err(err) = tokenizer_store.ensure_deepseek_fallback() {
        eprintln!("bootstrap: preload deepseek fallback tokenizer failed: {err}");
    }

    let state = Arc::new(AppState::new(AppStateInit {
        storage: storage.clone(),
        storage_writes: write_tx,
        http: Arc::new(http_client),
        spoof_http: Arc::new(spoof_http_client),
        global,
        providers: registry,
        tokenizers: tokenizer_store,
        users,
        keys,
    }));

    Ok(Bootstrap {
        config_path,
        config,
        storage,
        state,
        storage_write_worker: write_worker,
    })
}

fn merge_global_settings(config: &BootstrapConfig) -> GlobalSettings {
    let mut global = GlobalSettings::default();
    if let Some(host) = config.global.host.as_ref() {
        global.host = host.clone();
    }
    if let Some(port) = config.global.port {
        global.port = port;
    }
    if let Some(proxy) = config.global.proxy.as_ref() {
        global.proxy = Some(proxy.clone());
    }
    if let Some(hf_token) = config.global.hf_token.as_ref() {
        global.hf_token = Some(hf_token.clone());
    }
    if let Some(hf_url) = config.global.hf_url.as_ref() {
        global.hf_url = Some(hf_url.clone());
    }
    if let Some(admin_key) = config.global.admin_key.as_ref() {
        global.admin_key = admin_key.clone();
    }
    if let Some(mask) = config.global.mask_sensitive_info {
        global.mask_sensitive_info = mask;
    }

    let dsn_overridden = config.global.dsn.is_some();
    if let Some(data_dir) = config.global.data_dir.as_ref() {
        global.data_dir = data_dir.clone();
        if !dsn_overridden {
            let dir = global.data_dir.trim_end_matches('/');
            global.dsn = format!("sqlite://{dir}/gproxy.db?mode=rwc");
        }
    }
    if let Some(dsn) = config.global.dsn.as_ref() {
        global.dsn = dsn.clone();
    }
    global
}

fn apply_cli_env_overrides(config: &mut BootstrapConfig, args: &CliArgs) {
    if let Some(host) = &args.host {
        config.global.host = Some(host.clone());
    }
    if let Some(port) = args.port {
        config.global.port = Some(port);
    }
    if let Some(proxy) = &args.proxy {
        config.global.proxy = Some(proxy.clone());
    }
    if let Some(admin_key) = &args.admin_key {
        config.global.admin_key = Some(admin_key.clone());
    }
    if let Some(mask_sensitive_info) = args.mask_sensitive_info {
        config.global.mask_sensitive_info = Some(mask_sensitive_info);
    }
    if let Some(data_dir) = &args.data_dir {
        config.global.data_dir = Some(data_dir.clone());
    }
    if let Some(dsn) = &args.dsn {
        config.global.dsn = Some(dsn.clone());
    }

    if let Some(capacity) = args.storage_write_queue_capacity {
        config.runtime.storage_write_queue_capacity = capacity;
    }
    if let Some(max_batch_size) = args.storage_write_max_batch_size {
        config.runtime.storage_write_max_batch_size = max_batch_size;
    }
    if let Some(aggregate_window_ms) = args.storage_write_aggregate_window_ms {
        config.runtime.storage_write_aggregate_window_ms = aggregate_window_ms;
    }
}

fn normalize_proxy(proxy: Option<&str>) -> Option<&str> {
    proxy.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then_some(value)
    })
}

fn build_http_client(proxy: Option<&str>) -> Result<WreqClient> {
    let mut builder = WreqClient::builder()
        .connect_timeout(Duration::from_secs(CLIENT_CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(CLIENT_REQUEST_TIMEOUT_SECS))
        .read_timeout(Duration::from_secs(CLIENT_STREAM_IDLE_TIMEOUT_SECS));

    if let Some(proxy_url) = normalize_proxy(proxy) {
        builder = builder.proxy(Proxy::all(proxy_url)?);
    }

    Ok(builder.build()?)
}

fn build_claudecode_spoof_client(proxy: Option<&str>) -> Result<WreqClient> {
    let mut default_headers = HeaderMap::new();
    default_headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/plain, */*"),
    );
    default_headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    default_headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));

    let mut builder = WreqClient::builder()
        .connect_timeout(Duration::from_secs(CLIENT_CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(CLIENT_REQUEST_TIMEOUT_SECS))
        .read_timeout(Duration::from_secs(CLIENT_STREAM_IDLE_TIMEOUT_SECS))
        .cookie_store(true)
        .emulation(Emulation::Chrome136)
        .default_headers(default_headers);

    if let Some(proxy_url) = normalize_proxy(proxy) {
        builder = builder.proxy(Proxy::all(proxy_url)?);
    }

    Ok(builder.build()?)
}

fn build_provider_registry(config: &BootstrapConfig) -> ProviderRegistry {
    let mut registry = ProviderRegistry::default();
    for channel_cfg in &config.channels {
        if channel_cfg.id.trim().is_empty() {
            eprintln!("bootstrap: skip channel with empty id");
            continue;
        }

        let channel = ChannelId::parse(channel_cfg.id.trim());
        let settings = resolve_provider_settings(&channel, &channel_cfg.settings);
        let (credentials, channel_states) = dedupe_credentials(&channel, channel_cfg);

        let dispatch = channel_cfg
            .dispatch
            .clone()
            .unwrap_or_else(|| match &channel {
                ChannelId::Builtin(builtin) => ProviderDispatchTable::default_for_builtin(*builtin),
                ChannelId::Custom(_) => ProviderDispatchTable::default_for_custom(),
            });
        registry.upsert(ProviderDefinition {
            channel,
            dispatch,
            settings,
            credentials: ProviderCredentialState {
                credentials,
                channel_states,
            },
        });
    }
    registry
}

async fn seed_registry_providers(
    storage: &SeaOrmStorage,
    registry: &ProviderRegistry,
) -> Result<std::collections::HashMap<String, i64>> {
    let existing = storage
        .list_providers(&ProviderQuery {
            channel: Scope::All,
            name: Scope::All,
            enabled: Scope::All,
            limit: None,
        })
        .await
        .context("list existing providers for bootstrap seed")?;

    let mut id_by_channel = existing
        .iter()
        .map(|row| (row.channel.clone(), row.id))
        .collect::<std::collections::HashMap<_, _>>();
    let mut next_id = existing.iter().map(|row| row.id).max().unwrap_or(-1) + 1;

    // Builtin providers are always persisted for admin visibility.
    // Runtime enable/disable still follows configured registry.
    let mut provider_by_channel = std::collections::HashMap::new();
    for builtin in BUILTIN_CHANNELS {
        let channel_id = ChannelId::builtin(builtin);
        provider_by_channel.insert(
            channel_id.as_str().to_string(),
            (
                resolve_provider_settings(&channel_id, &serde_json::json!({})),
                ProviderDispatchTable::default_for_builtin(builtin),
            ),
        );
    }
    for provider in &registry.providers {
        provider_by_channel.insert(
            provider.channel.as_str().to_string(),
            (provider.settings.clone(), provider.dispatch.clone()),
        );
    }

    let mut batch = StorageWriteBatch::default();
    for (channel, (settings, dispatch)) in provider_by_channel {
        let id = if let Some(id) = id_by_channel.get(channel.as_str()).copied() {
            id
        } else {
            let id = next_id;
            next_id += 1;
            id_by_channel.insert(channel.clone(), id);
            id
        };

        let settings_json = provider_settings_to_json_string(&settings)
            .context("serialize provider settings for bootstrap seed")?;
        let dispatch_json = serde_json::to_string(&dispatch)
            .context("serialize provider dispatch for bootstrap seed")?;
        batch.apply(StorageWriteEvent::UpsertProvider(ProviderWrite {
            id,
            name: channel.clone(),
            channel,
            settings_json,
            dispatch_json,
            enabled: true,
        }));
    }

    if !batch.is_empty() {
        storage
            .write_batch(batch)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    }

    Ok(id_by_channel)
}

async fn seed_global_settings(storage: &SeaOrmStorage, global: &GlobalSettings) -> Result<()> {
    let mut batch = StorageWriteBatch::default();
    batch.apply(StorageWriteEvent::UpsertGlobalSettings(
        GlobalSettingsWrite {
            host: global.host.clone(),
            port: global.port,
            hf_token: global.hf_token.clone(),
            hf_url: global.hf_url.clone(),
            proxy: global.proxy.clone(),
            admin_key: global.admin_key.clone(),
            mask_sensitive_info: global.mask_sensitive_info,
            dsn: global.dsn.clone(),
            data_dir: global.data_dir.clone(),
        },
    ));

    storage
        .write_batch(batch)
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok(())
}

async fn seed_registry_credentials_and_statuses(
    storage: &SeaOrmStorage,
    registry: &mut ProviderRegistry,
    provider_ids: &std::collections::HashMap<String, i64>,
) -> Result<()> {
    let existing_credentials = storage
        .list_credentials(&CredentialQuery {
            provider_id: Scope::All,
            kind: Scope::All,
            enabled: Scope::All,
            limit: None,
        })
        .await
        .context("list existing credentials for bootstrap seed")?;
    let mut credential_id_by_provider_and_name = existing_credentials
        .iter()
        .filter_map(|row| {
            row.name
                .as_ref()
                .map(|name| ((row.provider_id, name.clone()), row.id))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut next_credential_id = existing_credentials
        .iter()
        .map(|row| row.id)
        .max()
        .unwrap_or(-1)
        + 1;

    let existing_statuses = storage
        .list_credential_statuses(&CredentialStatusQuery {
            id: Scope::All,
            credential_id: Scope::All,
            channel: Scope::All,
            health_kind: Scope::All,
            limit: None,
        })
        .await
        .context("list existing credential statuses for bootstrap seed")?;
    let mut status_id_by_credential_and_channel = existing_statuses
        .iter()
        .map(|row| ((row.credential_id, row.channel.clone()), row.id))
        .collect::<std::collections::HashMap<_, _>>();
    let mut next_status_id = existing_statuses
        .iter()
        .map(|row| row.id)
        .max()
        .unwrap_or(-1)
        + 1;

    let mut batch = StorageWriteBatch::default();
    for provider in &mut registry.providers {
        let channel = provider.channel.as_str().to_string();
        let Some(provider_id) = provider_ids.get(channel.as_str()).copied() else {
            continue;
        };

        let mut runtime_to_db_credential_ids = std::collections::HashMap::new();
        for credential in provider.credentials.credentials.iter_mut() {
            let credential_name = credential
                .label
                .clone()
                .unwrap_or_else(|| credential.id.to_string());
            let credential_id = if let Some(id) = credential_id_by_provider_and_name
                .get(&(provider_id, credential_name.clone()))
                .copied()
            {
                id
            } else {
                let id = next_credential_id;
                next_credential_id += 1;
                credential_id_by_provider_and_name
                    .insert((provider_id, credential_name.clone()), id);
                id
            };

            runtime_to_db_credential_ids.insert(credential.id, credential_id);
            credential.id = credential_id;

            let secret_json = serde_json::to_string(&credential.credential)
                .context("serialize credential secret_json for bootstrap seed")?;
            batch.apply(StorageWriteEvent::UpsertCredential(CredentialWrite {
                id: credential_id,
                provider_id,
                name: Some(credential_name),
                kind: credential_kind_for_storage(&credential.credential),
                settings_json: None,
                secret_json,
                enabled: true,
            }));
        }

        for state in &mut provider.credentials.channel_states {
            if let Some(db_id) = runtime_to_db_credential_ids
                .get(&state.credential_id)
                .copied()
            {
                state.credential_id = db_id;
            }
        }

        let state_by_credential_id = provider
            .credentials
            .channel_states
            .iter()
            .map(|state| (state.credential_id, state))
            .collect::<std::collections::HashMap<_, _>>();

        for credential in provider.credentials.list_credentials() {
            let credential_id = credential.id;

            let (health_kind, health_json, checked_at_unix_ms, last_error) = state_by_credential_id
                .get(&credential_id)
                .map(|state| {
                    let (kind, json) = credential_health_to_storage(&state.health);
                    (
                        kind,
                        json,
                        state
                            .checked_at_unix_ms
                            .map(|value| value.min(i64::MAX as u64) as i64),
                        state.last_error.clone(),
                    )
                })
                .unwrap_or_else(|| ("healthy".to_string(), None, None, None));

            let status_id = if let Some(id) = status_id_by_credential_and_channel
                .get(&(credential_id, channel.clone()))
                .copied()
            {
                id
            } else {
                let id = next_status_id;
                next_status_id += 1;
                status_id_by_credential_and_channel.insert((credential_id, channel.clone()), id);
                id
            };

            batch.apply(StorageWriteEvent::UpsertCredentialStatus(
                CredentialStatusWrite {
                    id: Some(status_id),
                    credential_id,
                    channel: channel.clone(),
                    health_kind,
                    health_json,
                    checked_at_unix_ms,
                    last_error,
                },
            ));
        }
    }

    if !batch.is_empty() {
        storage
            .write_batch(batch)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    }

    hydrate_registry_credentials_and_statuses(storage, registry, provider_ids)
        .await
        .context("hydrate registry credentials and statuses from storage")?;

    Ok(())
}

async fn hydrate_registry_credentials_and_statuses(
    storage: &SeaOrmStorage,
    registry: &mut ProviderRegistry,
    provider_ids: &std::collections::HashMap<String, i64>,
) -> Result<()> {
    let rows = storage
        .list_credentials(&CredentialQuery {
            provider_id: Scope::All,
            kind: Scope::All,
            enabled: Scope::All,
            limit: None,
        })
        .await
        .context("list credentials for runtime hydration")?;

    let statuses = storage
        .list_credential_statuses(&CredentialStatusQuery {
            id: Scope::All,
            credential_id: Scope::All,
            channel: Scope::All,
            health_kind: Scope::All,
            limit: None,
        })
        .await
        .context("list credential statuses for runtime hydration")?;
    let status_by_credential_and_channel = statuses
        .into_iter()
        .map(|item| ((item.credential_id, item.channel.clone()), item))
        .collect::<std::collections::HashMap<_, _>>();

    for provider in &mut registry.providers {
        let channel = provider.channel.as_str().to_string();
        let Some(provider_id) = provider_ids.get(channel.as_str()).copied() else {
            continue;
        };
        provider.credentials.credentials.clear();
        provider.credentials.channel_states.clear();

        for row in rows.iter().filter(|row| row.provider_id == provider_id) {
            let credential = serde_json::from_value::<ChannelCredential>(row.secret_json.clone())
                .with_context(|| {
                format!(
                    "deserialize credential secret_json id={} provider_id={}",
                    row.id, row.provider_id
                )
            })?;
            provider.credentials.upsert_credential(CredentialRef {
                id: row.id,
                label: row.name.clone(),
                credential,
            });

            if let Some(status) = status_by_credential_and_channel.get(&(row.id, channel.clone())) {
                provider
                    .credentials
                    .upsert_channel_state(ChannelCredentialState {
                        channel: provider.channel.clone(),
                        credential_id: row.id,
                        health: credential_health_from_storage(
                            status.health_kind.as_str(),
                            status.health_json.as_ref(),
                        )?,
                        checked_at_unix_ms: status.checked_at.and_then(|checked| {
                            let unix_ms = checked.unix_timestamp_nanos() / 1_000_000;
                            if unix_ms < 0 {
                                return None;
                            }
                            u64::try_from(unix_ms).ok()
                        }),
                        last_error: status.last_error.clone(),
                    });
            }
        }
    }

    Ok(())
}

fn credential_kind_for_storage(credential: &ChannelCredential) -> String {
    match credential {
        ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::OpenAi(_)) => {
            "builtin/openai"
        }
        ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::Claude(_)) => {
            "builtin/claude"
        }
        ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::AiStudio(_)) => {
            "builtin/aistudio"
        }
        ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::VertexExpress(_)) => {
            "builtin/vertexexpress"
        }
        ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::Vertex(_)) => {
            "builtin/vertex"
        }
        ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::GeminiCli(_)) => {
            "builtin/geminicli"
        }
        ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::ClaudeCode(_)) => {
            "builtin/claudecode"
        }
        ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::Codex(_)) => {
            "builtin/codex"
        }
        ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::Antigravity(_)) => {
            "builtin/antigravity"
        }
        ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::Nvidia(_)) => {
            "builtin/nvidia"
        }
        ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::Deepseek(_)) => {
            "builtin/deepseek"
        }
        ChannelCredential::Custom(_) => "custom/apikey",
    }
    .to_string()
}

fn credential_health_to_storage(health: &CredentialHealth) -> (String, Option<String>) {
    match health {
        CredentialHealth::Healthy => ("healthy".to_string(), None),
        CredentialHealth::Dead => ("dead".to_string(), None),
        CredentialHealth::Partial { models } => {
            ("partial".to_string(), serde_json::to_string(models).ok())
        }
    }
}

fn credential_health_from_storage(
    kind: &str,
    health_json: Option<&serde_json::Value>,
) -> Result<CredentialHealth> {
    match kind {
        "healthy" => Ok(CredentialHealth::Healthy),
        "dead" => Ok(CredentialHealth::Dead),
        "partial" => {
            let models = if let Some(value) = health_json {
                serde_json::from_value::<Vec<ModelCooldown>>(value.clone())
                    .context("deserialize partial health model cooldown list")?
            } else {
                Vec::new()
            };
            Ok(CredentialHealth::Partial { models })
        }
        _ => Ok(CredentialHealth::Healthy),
    }
}

async fn seed_admin_principal(
    storage: &SeaOrmStorage,
    user: UserWrite,
    key: UserKeyWrite,
) -> Result<()> {
    let mut batch = StorageWriteBatch::default();
    batch.apply(StorageWriteEvent::UpsertUser(user));
    batch.apply(StorageWriteEvent::UpsertUserKey(key));
    storage
        .write_batch(batch)
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok(())
}

fn resolve_provider_settings(
    channel: &ChannelId,
    settings: &serde_json::Value,
) -> gproxy_provider::ChannelSettings {
    parse_provider_settings_value_for_channel(channel, settings).unwrap_or_else(|err| {
        eprintln!(
            "bootstrap: invalid settings for channel={} ({err}), fallback to default",
            channel.as_str()
        );
        parse_provider_settings_value_for_channel(channel, &serde_json::json!({}))
            .unwrap_or_default()
    })
}

fn dedupe_credentials(
    channel: &ChannelId,
    channel_cfg: &crate::bootstrap::config::ChannelConfigFile,
) -> (Vec<CredentialRef>, Vec<ChannelCredentialState>) {
    let mut credentials = Vec::new();
    let mut channel_states = Vec::new();

    let mut seen_ids = HashSet::<i64>::new();
    let mut seen_fingerprints = HashSet::new();

    for (idx, credential_cfg) in channel_cfg.credentials.iter().enumerate() {
        let id = credential_cfg
            .id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or((idx as i64) + 1);
        if !seen_ids.insert(id) {
            eprintln!(
                "bootstrap: drop duplicate credential id channel={} id={}",
                channel_cfg.id, id
            );
            continue;
        }

        let fingerprint = credential_fingerprint(credential_cfg);
        if !seen_fingerprints.insert(fingerprint) {
            eprintln!(
                "bootstrap: drop duplicate credential material channel={} id={}",
                channel_cfg.id, id
            );
            continue;
        }

        let Some(credential) = build_channel_credential(channel, credential_cfg) else {
            eprintln!(
                "bootstrap: skip unsupported credential payload channel={} id={}",
                channel_cfg.id, id
            );
            continue;
        };

        credentials.push(CredentialRef {
            id,
            label: credential_cfg.label.clone(),
            credential,
        });

        if let Some(state_cfg) = credential_cfg.state.as_ref() {
            channel_states.push(ChannelCredentialState {
                channel: channel.clone(),
                credential_id: id,
                health: credential_health_from_config(&state_cfg.health),
                checked_at_unix_ms: state_cfg.checked_at_unix_ms,
                last_error: state_cfg.last_error.clone(),
            });
        }
    }

    (credentials, channel_states)
}

fn build_channel_credential(
    channel: &ChannelId,
    credential: &CredentialConfigFile,
) -> Option<ChannelCredential> {
    if let Some(builtin) = credential.builtin.clone() {
        if matches!(channel, ChannelId::Builtin(_)) {
            return Some(ChannelCredential::Builtin(builtin));
        }
        return None;
    }

    let secret = credential
        .secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    match channel {
        ChannelId::Custom(_) => Some(ChannelCredential::Custom(CustomChannelCredential {
            api_key: secret.to_string(),
        })),
        ChannelId::Builtin(builtin) => {
            let material = match builtin {
                BuiltinChannel::OpenAi => ChannelCredential::Builtin(
                    gproxy_provider::BuiltinChannelCredential::OpenAi(openai::OpenAiCredential {
                        api_key: secret.to_string(),
                    }),
                ),
                BuiltinChannel::Claude => ChannelCredential::Builtin(
                    gproxy_provider::BuiltinChannelCredential::Claude(claude::ClaudeCredential {
                        api_key: secret.to_string(),
                    }),
                ),
                BuiltinChannel::AiStudio => {
                    ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::AiStudio(
                        aistudio::AiStudioCredential {
                            api_key: secret.to_string(),
                        },
                    ))
                }
                BuiltinChannel::VertexExpress => ChannelCredential::Builtin(
                    gproxy_provider::BuiltinChannelCredential::VertexExpress(
                        vertexexpress::VertexExpressCredential {
                            api_key: secret.to_string(),
                        },
                    ),
                ),
                BuiltinChannel::Nvidia => ChannelCredential::Builtin(
                    gproxy_provider::BuiltinChannelCredential::Nvidia(nvidia::NvidiaCredential {
                        api_key: secret.to_string(),
                    }),
                ),
                BuiltinChannel::Deepseek => {
                    ChannelCredential::Builtin(gproxy_provider::BuiltinChannelCredential::Deepseek(
                        deepseek::DeepseekCredential {
                            api_key: secret.to_string(),
                        },
                    ))
                }
                BuiltinChannel::Vertex
                | BuiltinChannel::GeminiCli
                | BuiltinChannel::ClaudeCode
                | BuiltinChannel::Codex
                | BuiltinChannel::Antigravity => return None,
            };
            Some(material)
        }
    }
}

fn credential_fingerprint(credential: &CredentialConfigFile) -> String {
    if let Some(builtin) = credential.builtin.as_ref() {
        return format!(
            "builtin:{}",
            toml::to_string(builtin).unwrap_or_else(|_| "<serialize-error>".to_string())
        );
    }
    if let Some(secret) = credential.secret.as_ref() {
        return format!("secret:{}", secret.trim());
    }
    format!(
        "identity:{}:{}",
        credential.id.as_deref().map(str::trim).unwrap_or_default(),
        credential.label.as_deref().unwrap_or_default()
    )
}

fn credential_health_from_config(health: &CredentialHealthConfigFile) -> CredentialHealth {
    match health {
        CredentialHealthConfigFile::Healthy => CredentialHealth::Healthy,
        CredentialHealthConfigFile::Partial { models } => CredentialHealth::Partial {
            models: models.clone(),
        },
        CredentialHealthConfigFile::Dead => CredentialHealth::Dead,
    }
}

fn ensure_admin_principal(
    global: &mut GlobalSettings,
    users: &mut Vec<MemoryUser>,
    keys: &mut std::collections::HashMap<String, MemoryUserKey>,
) -> Result<(UserWrite, UserKeyWrite)> {
    const ADMIN_USER_ID: i64 = 0;
    let admin_user_id = if let Some(existing) = users.iter_mut().find(|row| row.id == ADMIN_USER_ID)
    {
        existing.name = "admin".to_string();
        existing.enabled = true;
        existing.id
    } else {
        let id = ADMIN_USER_ID;
        users.push(MemoryUser {
            id,
            name: "admin".to_string(),
            enabled: true,
        });
        id
    };

    let user_write = UserWrite {
        id: admin_user_id,
        name: "admin".to_string(),
        enabled: true,
    };

    let admin_key = if global.admin_key.trim().is_empty() {
        if let Some(existing) = find_existing_admin_api_key(keys, admin_user_id) {
            global.admin_key = existing.clone();
            existing
        } else {
            let generated = generate_16_digit_admin_key();
            eprintln!("bootstrap: generated admin api key: {generated}");
            global.admin_key = generated.clone();
            generated
        }
    } else {
        let normalized = global.admin_key.trim().to_string();
        global.admin_key = normalized.clone();
        normalized
    };

    let admin_key_id = keys
        .get(admin_key.as_str())
        .map(|row| row.id)
        .unwrap_or_else(|| next_incremental_key_id(keys));

    keys.insert(
        admin_key.clone(),
        MemoryUserKey {
            id: admin_key_id,
            user_id: admin_user_id,
            api_key: admin_key.clone(),
            enabled: true,
        },
    );

    let key_write = UserKeyWrite {
        id: admin_key_id,
        user_id: admin_user_id,
        api_key: admin_key,
        label: Some("bootstrap-admin-key".to_string()),
        enabled: true,
    };

    Ok((user_write, key_write))
}

fn find_existing_admin_api_key(
    keys: &std::collections::HashMap<String, MemoryUserKey>,
    admin_user_id: i64,
) -> Option<String> {
    keys.values()
        .filter(|row| row.user_id == admin_user_id)
        .min_by_key(|row| (!row.enabled, row.id))
        .map(|row| row.api_key.clone())
}

fn next_incremental_key_id(keys: &std::collections::HashMap<String, MemoryUserKey>) -> i64 {
    keys.values().map(|row| row.id).max().unwrap_or(-1) + 1
}

fn generate_16_digit_admin_key() -> String {
    const MIN: u64 = 1_000_000_000_000_000;
    const MAX: u64 = 10_000_000_000_000_000;
    rand::rng().random_range(MIN..MAX).to_string()
}
