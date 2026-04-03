use serde::{Deserialize, Serialize};

/// Server-wide configuration loaded from `global_settings` table or TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub admin_key: String,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default = "default_spoof_emulation")]
    pub spoof_emulation: String,
    #[serde(default = "default_update_source")]
    pub update_source: String,
    #[serde(default = "default_mask")]
    pub mask_sensitive_info: bool,
    pub dsn: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            admin_key: String::new(),
            proxy: None,
            spoof_emulation: default_spoof_emulation(),
            update_source: default_update_source(),
            mask_sensitive_info: true,
            dsn: String::new(),
            data_dir: default_data_dir(),
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    8787
}
fn default_spoof_emulation() -> String {
    "chrome_136".to_string()
}
fn default_update_source() -> String {
    "github".to_string()
}
fn default_mask() -> bool {
    true
}
fn default_data_dir() -> String {
    "./data".to_string()
}
