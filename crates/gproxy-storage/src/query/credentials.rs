use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use super::Scope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CredentialQuery {
    pub id: Scope<i64>,
    pub provider_id: Scope<i64>,
    pub kind: Scope<String>,
    pub enabled: Scope<bool>,
    pub name_contains: Option<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialQueryRow {
    pub id: i64,
    pub provider_id: i64,
    pub name: Option<String>,
    pub kind: String,
    pub settings_json: Option<Value>,
    pub secret_json: Value,
    pub enabled: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialQueryCount {
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CredentialStatusQuery {
    pub id: Scope<i64>,
    pub credential_id: Scope<i64>,
    pub provider_id: Scope<i64>,
    pub channel: Scope<String>,
    pub health_kind: Scope<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialStatusQueryRow {
    pub id: i64,
    pub credential_id: i64,
    pub channel: String,
    pub health_kind: String,
    pub health_json: Option<Value>,
    pub checked_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialStatusQueryCount {
    pub count: u64,
}
