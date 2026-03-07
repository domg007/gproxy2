use super::config::merge_global_settings_from_storage;
use super::principal::PrincipalCache;
use super::*;

pub(super) struct RuntimeStorage {
    pub(super) storage: Arc<SeaOrmStorage>,
    pub(super) tokenizer_cache_dir: std::path::PathBuf,
}

pub(super) struct StoragePreference {
    pub(super) global: GlobalSettings,
    pub(super) config_for_providers: BootstrapConfig,
}

fn ensure_runtime_directories(global: &GlobalSettings) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(&global.data_dir)
        .with_context(|| format!("create data dir {}", global.data_dir))?;
    let tokenizer_cache_dir = std::path::Path::new(&global.data_dir).join("tokenizers");
    std::fs::create_dir_all(&tokenizer_cache_dir).with_context(|| {
        format!(
            "create tokenizer cache dir {}",
            tokenizer_cache_dir.to_string_lossy()
        )
    })?;
    Ok(tokenizer_cache_dir)
}

pub(super) async fn init_runtime_storage(global: &GlobalSettings) -> Result<RuntimeStorage> {
    let tokenizer_cache_dir = ensure_runtime_directories(global)?;
    let storage = Arc::new(
        SeaOrmStorage::connect(&global.dsn)
            .await
            .with_context(|| format!("connect storage dsn={}", global.dsn))?,
    );
    storage.sync().await.context("sync storage schema")?;
    Ok(RuntimeStorage {
        storage,
        tokenizer_cache_dir,
    })
}

pub(super) async fn resolve_storage_preference(
    storage: &SeaOrmStorage,
    args: &CliArgs,
    config: &BootstrapConfig,
    mut global: GlobalSettings,
) -> Result<StoragePreference> {
    let bootstrap_force_config = args.bootstrap_force_config.unwrap_or(false);
    let should_prefer_storage = !bootstrap_force_config
        && storage_has_bootstrap_state(storage)
            .await
            .context("check bootstrap storage state")?;

    let mut config_for_providers = config.clone();
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
        config_for_providers.channels.clear();
        eprintln!(
            "bootstrap: storage is initialized; skip config-file channel/provider import (except admin_key override)"
        );
    }

    Ok(StoragePreference {
        global,
        config_for_providers,
    })
}

pub(super) fn spawn_storage_writer(
    config: &BootstrapConfig,
    storage: Arc<SeaOrmStorage>,
) -> (
    gproxy_storage::StorageWriteSender,
    tokio::task::JoinHandle<Result<(), StorageWriteSinkError>>,
) {
    let (write_tx, write_rx) = storage_write_channel(config.runtime.storage_write_queue_capacity);
    let worker = spawn_storage_write_worker(
        storage,
        write_rx,
        StorageWriteWorkerConfig {
            max_batch_size: config.runtime.storage_write_max_batch_size,
            aggregate_window: Duration::from_millis(
                config.runtime.storage_write_aggregate_window_ms,
            ),
        },
    );
    (write_tx, worker)
}

pub(super) fn build_app_state(
    storage: Arc<SeaOrmStorage>,
    storage_writes: gproxy_storage::StorageWriteSender,
    global: GlobalSettings,
    providers: ProviderRegistry,
    tokenizer_cache_dir: std::path::PathBuf,
    principals: PrincipalCache,
) -> Result<Arc<AppState>> {
    let http_client = build_http_client(global.proxy.as_deref())
        .context("build standard upstream http client")?;
    let spoof_http_client =
        build_claudecode_spoof_client(global.proxy.as_deref(), global.spoof_emulation.as_str())
            .context("build claudecode spoof http client")?;
    let tokenizer_store = Arc::new(LocalTokenizerStore::new(tokenizer_cache_dir));
    if let Err(err) = tokenizer_store.ensure_deepseek_fallback() {
        eprintln!("bootstrap: preload deepseek fallback tokenizer failed: {err}");
    }

    Ok(Arc::new(AppState::new(AppStateInit {
        storage,
        storage_writes,
        http: Arc::new(http_client),
        spoof_http: Arc::new(spoof_http_client),
        global,
        providers,
        tokenizers: tokenizer_store,
        users: principals.users,
        keys: principals.keys,
    })))
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
