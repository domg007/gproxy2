use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use gproxy_provider::{BuiltinChannelCredential, ModelCooldown, ProviderDispatchTable};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CONFIG_PATH: &str = "./gproxy.toml";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BootstrapConfig {
    #[serde(default)]
    pub global: GlobalConfigFile,
    #[serde(default)]
    pub runtime: RuntimeConfigFile,
    #[serde(default)]
    pub channels: Vec<ChannelConfigFile>,
}

impl BootstrapConfig {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            if is_default_config_path(path) {
                return Ok(Self::default());
            }
            return Err(anyhow::anyhow!(
                "bootstrap config not found: {}",
                path.display()
            ));
        }

        let text = std::fs::read_to_string(path)
            .with_context(|| format!("read bootstrap config {}", path.display()))?;
        let config = toml::from_str::<Self>(&text)
            .with_context(|| format!("parse bootstrap config {}", path.display()))?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalConfigFile {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub proxy: Option<String>,
    pub spoof_emulation: Option<String>,
    pub update_source: Option<String>,
    pub hf_token: Option<String>,
    pub hf_url: Option<String>,
    pub admin_key: Option<String>,
    pub mask_sensitive_info: Option<bool>,
    pub dsn: Option<String>,
    pub data_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfigFile {
    pub storage_write_queue_capacity: usize,
    pub storage_write_max_batch_size: usize,
    pub storage_write_aggregate_window_ms: u64,
}

impl Default for RuntimeConfigFile {
    fn default() -> Self {
        Self {
            storage_write_queue_capacity: 4096,
            storage_write_max_batch_size: 1024,
            storage_write_aggregate_window_ms: 25,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfigFile {
    pub id: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub settings: serde_json::Value,
    #[serde(default)]
    pub dispatch: Option<ProviderDispatchTable>,
    #[serde(default)]
    pub credentials: Vec<CredentialConfigFile>,
}

impl Default for ChannelConfigFile {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: true,
            settings: serde_json::json!({}),
            dispatch: None,
            credentials: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialConfigFile {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub builtin: Option<BuiltinChannelCredential>,
    #[serde(default)]
    pub state: Option<CredentialStateConfigFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CredentialStateConfigFile {
    #[serde(default)]
    pub health: CredentialHealthConfigFile,
    #[serde(default)]
    pub checked_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CredentialHealthConfigFile {
    #[default]
    Healthy,
    Partial {
        #[serde(default)]
        models: Vec<ModelCooldown>,
    },
    Dead,
}

fn default_enabled() -> bool {
    true
}

fn is_default_config_path(path: &Path) -> bool {
    let canonical_default = PathBuf::from(DEFAULT_CONFIG_PATH);
    path == canonical_default || path == Path::new("gproxy.toml")
}
