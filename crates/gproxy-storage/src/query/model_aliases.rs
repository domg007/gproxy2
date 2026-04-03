use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::Scope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModelAliasQuery {
    pub id: Scope<i64>,
    pub alias: Scope<String>,
    pub provider_id: Scope<i64>,
    pub enabled: Scope<bool>,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelAliasQueryRow {
    pub id: i64,
    pub alias: String,
    pub provider_id: i64,
    pub model_id: String,
    pub enabled: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}
