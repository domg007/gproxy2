//! Configuration: CLI/env-only bootstrap (no config file).

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::ValueEnum;

/// CLI input type only — used by `clap` for `--persistence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum PersistenceKind {
    /// Local disk — single-instance only.
    File,
    /// SeaORM-backed database — supports multi-instance.
    Db,
}

/// Validated cache configuration. Illegal states (e.g. Redis without URL)
/// cannot be constructed.
#[derive(Debug, Clone)]
pub enum CacheConfig {
    Memory,
    Redis { url: String },
}

impl CacheConfig {
    pub fn from_url(redis_url: Option<String>) -> Self {
        match redis_url {
            Some(url) => Self::Redis { url },
            None => Self::Memory,
        }
    }
}

/// Validated persistence configuration. `Db` variant always carries a DSN.
#[derive(Debug, Clone)]
pub enum PersistenceConfig {
    File { data_dir: PathBuf },
    Db { dsn: String },
}

impl PersistenceConfig {
    pub fn from_parts(
        kind: PersistenceKind,
        data_dir: PathBuf,
        dsn: Option<String>,
    ) -> anyhow::Result<Self> {
        match kind {
            PersistenceKind::File => Ok(Self::File { data_dir }),
            PersistenceKind::Db => Ok(Self::Db {
                dsn: dsn
                    .ok_or_else(|| anyhow::anyhow!("db persistence requires --dsn / GPROXY_DSN"))?,
            }),
        }
    }
}

/// Immutable runtime snapshot built from CLI args / environment variables.
///
/// Wrapped in [`Arc`](std::sync::Arc) for cheap sharing across handlers.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Bind host. IPv6 addresses must use bracket notation (e.g. `[::1]`)
    /// because [`bind_addr`](Self::bind_addr) parses `host:port` as a
    /// [`SocketAddr`].
    pub host: String,
    pub port: u16,
    pub cache: CacheConfig,
    pub persistence: PersistenceConfig,
    /// Numeric identifier for this instance. Numeric (not a name) so the
    /// database can partition / shard per-instance rows by it later.
    pub instance_id: u64,
}

impl RuntimeConfig {
    /// Resolve the `host:port` bind address.
    pub fn bind_addr(&self) -> anyhow::Result<SocketAddr> {
        let addr = format!("{}:{}", self.host, self.port);
        addr.parse()
            .map_err(|e| anyhow::anyhow!("invalid bind address {addr}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_cfg() -> RuntimeConfig {
        RuntimeConfig {
            host: "127.0.0.1".to_string(),
            port: 8787,
            cache: CacheConfig::Memory,
            persistence: PersistenceConfig::File {
                data_dir: PathBuf::from("./data"),
            },
            instance_id: 0,
        }
    }

    #[test]
    fn bind_addr_parses() {
        let addr = file_cfg().bind_addr().unwrap();
        assert_eq!(addr.to_string(), "127.0.0.1:8787");
    }

    #[test]
    fn persistence_db_without_dsn_is_err() {
        let err = PersistenceConfig::from_parts(PersistenceKind::Db, PathBuf::from("./data"), None)
            .unwrap_err();
        assert!(err.to_string().contains("GPROXY_DSN"));
    }

    #[test]
    fn persistence_db_with_dsn_is_ok() {
        PersistenceConfig::from_parts(
            PersistenceKind::Db,
            PathBuf::from("./data"),
            Some("sqlite://test.db".to_string()),
        )
        .unwrap();
    }

    #[test]
    fn persistence_file_is_ok() {
        PersistenceConfig::from_parts(PersistenceKind::File, PathBuf::from("./data"), None)
            .unwrap();
    }

    #[test]
    fn cache_from_url_none_is_memory() {
        assert!(matches!(CacheConfig::from_url(None), CacheConfig::Memory));
    }

    #[test]
    fn cache_from_url_some_is_redis() {
        let cfg = CacheConfig::from_url(Some("redis://127.0.0.1".to_string()));
        assert!(matches!(cfg, CacheConfig::Redis { .. }));
    }
}
