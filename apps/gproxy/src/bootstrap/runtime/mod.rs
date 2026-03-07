use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use gproxy_admin::{MemoryUser, MemoryUserKey};
use gproxy_core::{
    AppState, AppStateInit, GlobalSettings, build_claudecode_spoof_client, build_http_client,
    normalize_spoof_emulation, normalize_update_source,
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

mod config;
mod principal;
mod registry;
mod storage;
mod storage_seed;

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
    let loaded = config::load_bootstrap_config(&args)?;
    let runtime_storage = storage::init_runtime_storage(
        &loaded.global,
        args.database_secret_key.as_deref(),
    )
    .await?;
    let preference = storage::resolve_storage_preference(
        runtime_storage.storage.as_ref(),
        &args,
        &loaded.config,
        loaded.global,
    )
    .await?;
    let (write_tx, write_worker) =
        storage::spawn_storage_writer(&loaded.config, runtime_storage.storage.clone());
    let registry = registry::build_seeded_provider_registry(
        runtime_storage.storage.as_ref(),
        &preference.config_for_providers,
    )
    .await?;
    let mut principals = principal::load_principal_cache(runtime_storage.storage.as_ref()).await?;
    let global = principal::seed_bootstrap_state(
        runtime_storage.storage.as_ref(),
        preference.global,
        &mut principals,
    )
    .await?;
    let state = storage::build_app_state(
        runtime_storage.storage.clone(),
        write_tx,
        global,
        registry,
        runtime_storage.tokenizer_cache_dir,
        principals,
    )?;

    Ok(Bootstrap {
        config_path: loaded.config_path,
        config: loaded.config,
        storage: runtime_storage.storage,
        state,
        storage_write_worker: write_worker,
    })
}
