use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum GlobalConfigError {
    #[error("missing required global config field: {0}")]
    MissingField(&'static str),
}

/// Final, merged global configuration used by the running process.
///
/// Merge order (after DB connection): CLI > ENV > DB, then persist back to DB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalConfig {
    pub host: String,
    pub port: u16,
    /// Stored as a hash (not plaintext).
    pub admin_key_hash: String,
    /// Optional outbound proxy (for upstream egress).
    pub proxy: Option<String>,
    /// Database DSN used for this process.
    pub dsn: String,
    /// Whether to redact sensitive fields in emitted events.
    pub event_redact_sensitive: bool,
}

/// Optional layer used for merging global config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalConfigPatch {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub admin_key_hash: Option<String>,
    pub proxy: Option<String>,
    pub dsn: Option<String>,
    pub event_redact_sensitive: Option<bool>,
}

impl GlobalConfigPatch {
    pub fn overlay(&mut self, other: GlobalConfigPatch) {
        if other.host.is_some() {
            self.host = other.host;
        }
        if other.port.is_some() {
            self.port = other.port;
        }
        if other.admin_key_hash.is_some() {
            self.admin_key_hash = other.admin_key_hash;
        }
        if other.proxy.is_some() {
            self.proxy = other.proxy;
        }
        if other.dsn.is_some() {
            self.dsn = other.dsn;
        }
        if other.event_redact_sensitive.is_some() {
            self.event_redact_sensitive = other.event_redact_sensitive;
        }
    }

    pub fn into_config(self) -> Result<GlobalConfig, GlobalConfigError> {
        Ok(GlobalConfig {
            host: self.host.unwrap_or_else(|| "0.0.0.0".to_string()),
            port: self.port.unwrap_or(8787),
            admin_key_hash: self
                .admin_key_hash
                .ok_or(GlobalConfigError::MissingField("admin_key_hash"))?,
            proxy: self.proxy,
            dsn: self.dsn.ok_or(GlobalConfigError::MissingField("dsn"))?,
            event_redact_sensitive: self.event_redact_sensitive.unwrap_or(true),
        })
    }
}

impl From<GlobalConfig> for GlobalConfigPatch {
    fn from(value: GlobalConfig) -> Self {
        Self {
            host: Some(value.host),
            port: Some(value.port),
            admin_key_hash: Some(value.admin_key_hash),
            proxy: value.proxy,
            dsn: Some(value.dsn),
            event_redact_sensitive: Some(value.event_redact_sensitive),
        }
    }
}
