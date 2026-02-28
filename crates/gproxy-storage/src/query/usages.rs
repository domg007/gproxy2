use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::Scope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UsageQuery {
    pub channel: Scope<String>,
    pub model: Scope<String>,
    pub user_id: Scope<i64>,
    pub user_key_id: Scope<i64>,
    pub from_unix_ms: Option<i64>,
    pub to_unix_ms: Option<i64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageQueryRow {
    pub trace_id: i64,
    pub at: OffsetDateTime,
    pub provider_id: Option<i64>,
    pub provider_channel: Option<String>,
    pub credential_id: Option<i64>,
    pub user_id: Option<i64>,
    pub user_key_id: Option<i64>,
    pub operation: String,
    pub protocol: String,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_input_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UsageSummary {
    pub count: u64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
}
