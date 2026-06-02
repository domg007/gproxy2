//! Configuration: CLI/env-only bootstrap (no config file).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use arc_swap::ArcSwap;
use clap::ValueEnum;

/// Where to persist durable data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum PersistenceKind {
    /// Local disk — single-instance only.
    File,
    /// SeaORM-backed database — supports multi-instance.
    Db,
}

/// Immutable runtime snapshot built from CLI args / environment variables.
/// Wrapped in [`ArcSwap`] so it can be hot-reloaded without locking readers.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Bind host. IPv6 addresses must use bracket notation (e.g. `[::1]`)
    /// because [`bind_addr`](Self::bind_addr) parses `host:port` as a [`SocketAddr`].
    pub host: String,
    pub port: u16,
    pub persistence: PersistenceKind,
    pub data_dir: PathBuf,
    /// Database connection string — required when `persistence == Db`.
    pub dsn: Option<String>,
    pub instance_name: String,
}

impl RuntimeConfig {
    /// Resolve the `host:port` bind address.
    pub fn bind_addr(&self) -> anyhow::Result<SocketAddr> {
        let addr = format!("{}:{}", self.host, self.port);
        addr.parse()
            .map_err(|e| anyhow::anyhow!("invalid bind address {addr}: {e}"))
    }

    /// Validate semantic constraints across fields.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.persistence == PersistenceKind::Db && self.dsn.is_none() {
            anyhow::bail!("db persistence requires --dsn / GPROXY_DSN");
        }
        Ok(())
    }
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

    fn base_cfg() -> RuntimeConfig {
        RuntimeConfig {
            host: "127.0.0.1".to_string(),
            port: 8787,
            persistence: PersistenceKind::File,
            data_dir: PathBuf::from("./data"),
            dsn: None,
            instance_name: "default".to_string(),
        }
    }

    #[test]
    fn bind_addr_parses() {
        let addr = base_cfg().bind_addr().unwrap();
        assert_eq!(addr.to_string(), "127.0.0.1:8787");
    }

    #[test]
    fn validate_rejects_db_without_dsn() {
        let cfg = RuntimeConfig {
            persistence: PersistenceKind::Db,
            ..base_cfg()
        };
        let err = cfg.validate().unwrap_err();
        assert!(err.to_string().contains("GPROXY_DSN"));
    }

    #[test]
    fn validate_accepts_db_with_dsn() {
        let cfg = RuntimeConfig {
            persistence: PersistenceKind::Db,
            dsn: Some("sqlite://test.db".to_string()),
            ..base_cfg()
        };
        cfg.validate().unwrap();
    }
}
