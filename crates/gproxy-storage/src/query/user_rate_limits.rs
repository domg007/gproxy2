use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::Scope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UserRateLimitQuery {
    pub id: Scope<i64>,
    pub user_id: Scope<i64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRateLimitQueryRow {
    pub id: i64,
    pub user_id: i64,
    pub model_pattern: String,
    pub rpm: Option<i32>,
    pub rpd: Option<i32>,
    pub total_tokens: Option<i64>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserQuotaRow {
    pub user_id: i64,
    pub tokens_used: i64,
    pub cost_used: f64,
    pub updated_at: OffsetDateTime,
}
