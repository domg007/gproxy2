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
    /// Whether to extract and record token usage from responses.
    #[serde(default = "default_true")]
    pub enable_usage: bool,
    /// Whether to record upstream request/response metadata.
    #[serde(default = "default_false")]
    pub enable_upstream_log: bool,
    /// Whether upstream logs include request/response body.
    #[serde(default = "default_false")]
    pub enable_upstream_log_body: bool,
    /// Whether to record downstream request/response metadata.
    #[serde(default = "default_false")]
    pub enable_downstream_log: bool,
    /// Whether downstream logs include request/response body.
    #[serde(default = "default_false")]
    pub enable_downstream_log_body: bool,
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
            enable_usage: true,
            enable_upstream_log: false,
            enable_upstream_log_body: false,
            enable_downstream_log: false,
            enable_downstream_log_body: false,
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
fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_data_dir() -> String {
    "./data".to_string()
}
