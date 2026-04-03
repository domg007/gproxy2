use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::Scope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UserModelPermissionQuery {
    pub id: Scope<i64>,
    pub user_id: Scope<i64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserModelPermissionQueryRow {
    pub id: i64,
    pub user_id: i64,
    pub model_pattern: String,
    pub created_at: OffsetDateTime,
}
