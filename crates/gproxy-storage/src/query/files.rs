use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use super::Scope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UserCredentialFileQuery {
    pub user_id: Scope<i64>,
    pub user_key_id: Scope<i64>,
    pub provider_id: Scope<i64>,
    pub credential_id: Scope<i64>,
    pub file_id: Scope<String>,
    pub active: Scope<bool>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserCredentialFileQueryRow {
    pub id: i64,
    pub user_id: i64,
    pub user_key_id: i64,
    pub provider_id: i64,
    pub credential_id: i64,
    pub file_id: String,
    pub active: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ClaudeFileQuery {
    pub provider_id: Scope<i64>,
    pub file_id: Scope<String>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaudeFileQueryRow {
    pub id: i64,
    pub provider_id: i64,
    pub file_id: String,
    pub file_created_at: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub downloadable: Option<bool>,
    pub raw_json: Value,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}
