use super::*;

pub(super) struct LoadedBootstrapConfig {
    pub(super) config_path: std::path::PathBuf,
    pub(super) config: BootstrapConfig,
    pub(super) global: GlobalSettings,
}

pub(super) fn load_bootstrap_config(args: &CliArgs) -> Result<LoadedBootstrapConfig> {
    let config_path = args.config.clone();
    let use_in_memory_defaults =
        !config_path.exists() && config_path == std::path::Path::new(DEFAULT_CONFIG_PATH);
    let mut config = BootstrapConfig::load(&config_path)?;
    apply_cli_env_overrides(&mut config, args);
    if use_in_memory_defaults {
        eprintln!(
            "bootstrap: {} not found, using in-memory defaults",
            DEFAULT_CONFIG_PATH
        );
    }

    let global = merge_global_settings(&config);
    validate_global_settings(&global)?;

    Ok(LoadedBootstrapConfig {
        config_path,
        config,
        global,
    })
}

fn validate_global_settings(global: &GlobalSettings) -> Result<()> {
    if global.data_dir.trim().is_empty() {
        return Err(anyhow::anyhow!("global.data_dir cannot be empty"));
    }
    if global.dsn.trim().is_empty() {
        return Err(anyhow::anyhow!("global.dsn cannot be empty"));
    }
    Ok(())
}

pub(super) fn merge_global_settings(config: &BootstrapConfig) -> GlobalSettings {
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
    if let Some(update_source) = config.global.update_source.as_deref() {
        global.update_source = normalize_update_source(Some(update_source));
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

pub(super) fn merge_global_settings_from_storage(
    mut current: GlobalSettings,
    row: &gproxy_storage::GlobalSettingsRow,
    admin_key_override: Option<&str>,
) -> GlobalSettings {
    current.host = row.host.clone();
    current.port = u16::try_from(row.port).unwrap_or(current.port);
    current.proxy = row.proxy.clone();
    current.spoof_emulation = normalize_spoof_emulation(row.spoof_emulation.as_deref());
    current.update_source = normalize_update_source(row.update_source.as_deref());
    current.hf_token = row.hf_token.clone();
    current.hf_url = row.hf_url.clone();
    current.mask_sensitive_info = row.mask_sensitive_info;
    current.admin_key = admin_key_override
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| row.admin_key.clone());
    current
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
