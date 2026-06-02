//! Configuration: on-disk TOML model + hot-swappable runtime snapshot.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::Deserialize;

/// On-disk configuration file model (TOML).
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub global: GlobalConfig,
}

/// `[global]` table of the config file.
#[derive(Debug, Clone, Deserialize)]
pub struct GlobalConfig {
    /// Bind host. IPv6 addresses must use bracket notation (e.g. `[::1]`)
    /// because `bind_addr()` parses `host:port` as a `SocketAddr`.
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    /// Database connection string. Parsed now, used in a later phase.
    pub dsn: Option<String>,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    8787
}
fn default_data_dir() -> PathBuf {
    PathBuf::from("./data")
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            data_dir: default_data_dir(),
            dsn: None,
        }
    }
}

/// Immutable runtime snapshot derived from [`ConfigFile`]. Wrapped in
/// `ArcSwap` so it can be hot-reloaded without locking readers.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub dsn: Option<String>,
}

impl RuntimeConfig {
    /// Resolve the `host:port` bind address.
    pub fn bind_addr(&self) -> anyhow::Result<SocketAddr> {
        let addr = format!("{}:{}", self.host, self.port);
        addr.parse()
            .map_err(|e| anyhow::anyhow!("invalid bind address {addr}: {e}"))
    }
}

impl From<ConfigFile> for RuntimeConfig {
    fn from(f: ConfigFile) -> Self {
        Self {
            host: f.global.host,
            port: f.global.port,
            data_dir: f.global.data_dir,
            dsn: f.global.dsn,
        }
    }
}

/// Parse a config from a TOML string.
pub fn parse_config(toml_str: &str) -> anyhow::Result<RuntimeConfig> {
    let file: ConfigFile = toml::from_str(toml_str)?;
    Ok(file.into())
}

/// Load configuration from a path on disk.
pub fn load_config(path: &Path) -> anyhow::Result<RuntimeConfig> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read config {}: {e}", path.display()))?;
    parse_config(&raw)
}

/// Shared, hot-swappable config handle.
pub type SharedConfig = Arc<ArcSwap<RuntimeConfig>>;

/// Wrap a runtime config in a shared, swappable handle.
pub fn shared(config: RuntimeConfig) -> SharedConfig {
    Arc::new(ArcSwap::from_pointee(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_and_applies_defaults() {
        let cfg = parse_config("[global]\n").expect("parse");
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.port, 8787);
        assert_eq!(cfg.data_dir, PathBuf::from("./data"));
        assert!(cfg.dsn.is_none());
    }

    #[test]
    fn overrides_and_bind_addr() {
        let cfg = parse_config("[global]\nhost = \"0.0.0.0\"\nport = 9000\ndsn = \"sqlite://x\"\n")
            .expect("parse");
        assert_eq!(cfg.port, 9000);
        assert_eq!(cfg.dsn.as_deref(), Some("sqlite://x"));
        assert_eq!(cfg.bind_addr().unwrap().to_string(), "0.0.0.0:9000");
    }

    #[test]
    fn empty_input_uses_defaults() {
        let cfg = parse_config("").unwrap();
        assert_eq!(cfg.port, 8787);
    }
}
