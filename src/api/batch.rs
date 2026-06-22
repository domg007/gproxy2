//! 批量端点共享逻辑:请求 DTO + 校验 + 分发(edge 与 native dispatcher 复用)。
use std::collections::HashSet;

use crate::api::error::ApiError;
use crate::app::AppState;
use crate::store::persistence::batch::{self, AdminEntity, BatchOutcome};

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BatchOp {
    Enable,
    Disable,
    Delete,
}

#[derive(Debug, serde::Deserialize)]
pub struct BatchRequest {
    pub op: BatchOp,
    pub ids: Vec<i64>,
}

/// 校验 + 去重 + 分发到持久化批量编排。删除尽力而为;启停定向更新。
pub async fn run_batch(
    state: &AppState,
    entity: AdminEntity,
    req: BatchRequest,
) -> Result<BatchOutcome, ApiError> {
    if req.ids.is_empty() {
        return Err(ApiError::BadRequest("ids must not be empty".into()));
    }
    let mut seen = HashSet::new();
    let ids: Vec<i64> = req.ids.into_iter().filter(|id| seen.insert(*id)).collect();

    let be = state.persistence.as_ref();
    let outcome = match req.op {
        BatchOp::Delete => batch::run_batch_delete(be, entity, &ids).await,
        BatchOp::Enable | BatchOp::Disable => {
            if !entity.supports_enable() {
                return Err(ApiError::BadRequest(
                    "entity does not support enable/disable".into(),
                ));
            }
            let enabled = matches!(req.op, BatchOp::Enable);
            batch::run_batch_set_enabled(be, entity, &ids, enabled).await
        }
    };
    Ok(outcome)
}
