use async_trait::async_trait;
use time::OffsetDateTime;

use gproxy_common::GlobalConfig;
use gproxy_provider_core::Event;

use crate::snapshot::{GlobalConfigRow, StorageSnapshot};

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("db error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("serde json error: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct UsageAggregateFilter {
    pub from: OffsetDateTime,
    pub to: OffsetDateTime,
    pub provider: Option<String>,
    pub credential_id: Option<i64>,
    pub model: Option<String>,
    pub model_contains: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UsageAggregate {
    pub matched_rows: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub total_tokens: i64,
}

/// Storage is used for:
/// - bootstrap (load_snapshot)
/// - admin mutations (writes only)
/// - event persistence (append_event)
///
/// Runtime reads must NOT hit DB; they read from in-memory snapshots.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Entity-first schema sync (SeaORM 2.0). Enabled by default at bootstrap.
    async fn sync(&self) -> StorageResult<()>;

    async fn load_global_config(&self) -> StorageResult<Option<GlobalConfigRow>>;
    async fn upsert_global_config(&self, config: &GlobalConfig) -> StorageResult<()>;

    async fn load_snapshot(&self) -> StorageResult<StorageSnapshot>;

    // Providers
    async fn upsert_provider(
        &self,
        name: &str,
        config_json: &serde_json::Value,
        enabled: bool,
    ) -> StorageResult<i64>;
    async fn delete_provider(&self, name: &str) -> StorageResult<()>;

    // Credentials
    async fn insert_credential(
        &self,
        provider_name: &str,
        name: Option<&str>,
        settings_json: &serde_json::Value,
        secret_json: &serde_json::Value,
        enabled: bool,
    ) -> StorageResult<i64>;
    async fn update_credential(
        &self,
        credential_id: i64,
        name: Option<&str>,
        settings_json: &serde_json::Value,
        secret_json: &serde_json::Value,
    ) -> StorageResult<()>;
    async fn set_credential_enabled(&self, credential_id: i64, enabled: bool) -> StorageResult<()>;
    async fn delete_credential(&self, credential_id: i64) -> StorageResult<()>;

    // Users / keys (auth)
    async fn upsert_user_by_id(&self, user_id: i64, name: &str, enabled: bool)
    -> StorageResult<()>;
    async fn set_user_enabled(&self, user_id: i64, enabled: bool) -> StorageResult<()>;
    async fn delete_user(&self, user_id: i64) -> StorageResult<()>;
    async fn insert_user_key(
        &self,
        user_id: i64,
        api_key: &str,
        label: Option<&str>,
        enabled: bool,
    ) -> StorageResult<i64>;
    async fn set_user_key_enabled(&self, user_key_id: i64, enabled: bool) -> StorageResult<()>;
    async fn update_user_key_label(
        &self,
        user_key_id: i64,
        label: Option<&str>,
    ) -> StorageResult<()>;
    async fn delete_user_key(&self, user_key_id: i64) -> StorageResult<()>;

    async fn append_event(&self, event: &Event) -> StorageResult<()>;

    async fn aggregate_usage_tokens(
        &self,
        filter: UsageAggregateFilter,
    ) -> StorageResult<UsageAggregate>;
}
