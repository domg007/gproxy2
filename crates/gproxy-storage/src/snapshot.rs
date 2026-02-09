use gproxy_common::GlobalConfig;
use serde_json::Value as JsonValue;
use time::OffsetDateTime;

#[derive(Debug, Clone)]
pub struct GlobalConfigRow {
    pub id: i64,
    pub config: GlobalConfig,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct ProviderRow {
    pub id: i64,
    pub name: String,
    pub config_json: JsonValue,
    pub enabled: bool,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct CredentialRow {
    pub id: i64,
    pub provider_id: i64,
    pub name: Option<String>,
    pub settings_json: JsonValue,
    pub secret_json: JsonValue,
    pub enabled: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct UserKeyRow {
    pub id: i64,
    pub user_id: i64,
    pub api_key: String,
    pub label: Option<String>,
    pub enabled: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct StorageSnapshot {
    pub global_config: Option<GlobalConfigRow>,
    pub providers: Vec<ProviderRow>,
    pub credentials: Vec<CredentialRow>,
    pub users: Vec<UserRow>,
    pub user_keys: Vec<UserKeyRow>,
}
