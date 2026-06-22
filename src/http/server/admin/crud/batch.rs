//! `POST /admin/batch/{entity}` — 批量启用/禁用/删除(native axum)。
use axum::Json;
use axum::extract::{Path, State};

use crate::admin::invalidate;
use crate::api::batch::{BatchRequest, run_batch};
use crate::api::error::ApiError;
use crate::app::AppState;
use crate::store::persistence::batch::{AdminEntity, BatchOutcome};

pub async fn batch(
    State(state): State<AppState>,
    Path(entity): Path<String>,
    Json(req): Json<BatchRequest>,
) -> Result<Json<BatchOutcome>, ApiError> {
    let entity = AdminEntity::from_seg(&entity)
        .ok_or_else(|| ApiError::NotFound("unknown entity".into()))?;
    let outcome = run_batch(&state, entity, req).await?;
    invalidate(&state).await;
    Ok(Json(outcome))
}
