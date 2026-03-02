use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use gproxy_admin::{MemoryUser, MemoryUserKey};
use gproxy_core::{
    AppState, AppStateInit, GlobalSettings, build_claudecode_spoof_client, build_http_client,
    normalize_spoof_emulation,
};
use gproxy_provider::{
    ChannelCredential, ChannelCredentialState, ChannelId, CredentialHealth, CredentialRef,
    LocalTokenizerStore, ProviderCredentialState, ProviderDefinition, ProviderDispatchTable,
    ProviderRegistry, parse_credential_pick_mode_from_provider_settings_value,
    parse_provider_settings_value_for_channel,
};
use gproxy_storage::{
    ProviderQuery, Scope, SeaOrmStorage, StorageWriteSinkError, StorageWriteWorkerConfig,
    UserKeyQuery, UserQuery, spawn_storage_write_worker, storage_write_channel,
};

use crate::bootstrap::cli::CliArgs;
use crate::bootstrap::config::{
    BootstrapConfig, CredentialConfigFile, CredentialHealthConfigFile, DEFAULT_CONFIG_PATH,
};

mod principal;
mod storage_seed;

use self::principal::{ensure_admin_principal, seed_admin_principal};
use self::storage_seed::{
    seed_global_settings, seed_registry_credentials_and_statuses, seed_registry_providers,
};

pub struct Bootstrap {
    pub config_path: std::path::PathBuf,
    pub config: BootstrapConfig,
    pub storage: Arc<SeaOrmStorage>,
    pub state: Arc<AppState>,
    pub storage_write_worker: tokio::task::JoinHandle<Result<(), StorageWriteSinkError>>,
}

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

    let bootstrap_force_config = args.bootstrap_force_config.unwrap_or(false);
    let should_prefer_storage = !bootstrap_force_config
        && storage_has_bootstrap_state(storage.as_ref())
            .await
            .context("check bootstrap storage state")?;
    if should_prefer_storage {
        if let Some(stored_global) = storage
            .get_global_settings()
            .await
            .context("load global settings from storage")?
        {
            let admin_key_override = config
                .global
                .admin_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            global = merge_global_settings_from_storage(global, &stored_global, admin_key_override);
        }
        eprintln!(
            "bootstrap: storage is initialized; skip config-file channel/provider import (except admin_key override)"
        );
    }

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

    let mut config_for_providers = config.clone();
    if should_prefer_storage {
        config_for_providers.channels.clear();
    }

    let mut registry = build_provider_registry(&config_for_providers);
    let provider_ids = seed_registry_providers(&storage, &mut registry)
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
            password: row.password,
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
    let spoof_http_client =
        build_claudecode_spoof_client(global.proxy.as_deref(), global.spoof_emulation.as_str())
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
    global.spoof_emulation = normalize_spoof_emulation(config.global.spoof_emulation.as_deref());
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

fn merge_global_settings_from_storage(
    mut current: GlobalSettings,
    row: &gproxy_storage::GlobalSettingsRow,
    admin_key_override: Option<&str>,
) -> GlobalSettings {
    current.host = row.host.clone();
    current.port = u16::try_from(row.port).unwrap_or(current.port);
    current.proxy = row.proxy.clone();
    current.spoof_emulation = normalize_spoof_emulation(row.spoof_emulation.as_deref());
    current.hf_token = row.hf_token.clone();
    current.hf_url = row.hf_url.clone();
    current.mask_sensitive_info = row.mask_sensitive_info;
    current.admin_key = admin_key_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| row.admin_key.clone());
    current
}

async fn storage_has_bootstrap_state(storage: &SeaOrmStorage) -> Result<bool> {
    let providers = storage
        .list_providers(&ProviderQuery {
            channel: Scope::All,
            name: Scope::All,
            enabled: Scope::All,
            limit: Some(1),
        })
        .await
        .context("list providers for bootstrap state check")?;
    if !providers.is_empty() {
        return Ok(true);
    }

    Ok(storage
        .get_global_settings()
        .await
        .context("load global settings for bootstrap state check")?
        .is_some())
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

fn build_provider_registry(config: &BootstrapConfig) -> ProviderRegistry {
    let mut registry = ProviderRegistry::default();
    for channel_cfg in &config.channels {
        if channel_cfg.id.trim().is_empty() {
            eprintln!("bootstrap: skip channel with empty id");
            continue;
        }

        let channel = ChannelId::parse(channel_cfg.id.trim());
        let settings = resolve_provider_settings(&channel, &channel_cfg.settings);
        let credential_pick_mode =
            parse_credential_pick_mode_from_provider_settings_value(&channel_cfg.settings);
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
            credential_pick_mode,
            credentials: ProviderCredentialState {
                credentials,
                channel_states,
            },
        });
    }
    registry
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

    gproxy_provider::credential_from_secret(channel, secret)
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
