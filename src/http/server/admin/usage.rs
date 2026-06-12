//! Read-only admin observability endpoints (§15, native-only): usage rows,
//! usage rollups, persisted credential health, and the request logs.
//!
//! All handlers are read-only and serialize their records directly — none of
//! `Usage` / `UsageRollup` / `CredentialStatus` / `DownstreamRequest` /
//! `UpstreamRequest` carries a secret. The log records' `headers_json` / `body`
//! are §14.3-redacted at capture, so reading them back is safe. Mounted behind
//! `require_admin`.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

use super::crud::internal;
use crate::api::error::ApiError;
use crate::app::AppState;
use crate::store::persistence::records::{
    AuditLog, CredentialStatus, DownstreamRequest, UpstreamRequest, Usage, UsageRollup,
};

/// `?limit=N` for the usage listing; defaults to 100, capped at 1000.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct UsageQuery {
    pub limit: Option<u64>,
}

const DEFAULT_USAGE_LIMIT: u64 = 100;
const MAX_USAGE_LIMIT: u64 = 1000;

/// `GET /admin/usage?limit=N` — the most recent usage rows (id desc).
pub async fn list_usage(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<Vec<Usage>>, ApiError> {
    let limit = q.limit.unwrap_or(DEFAULT_USAGE_LIMIT).min(MAX_USAGE_LIMIT);
    Ok(Json(
        state
            .persistence
            .list_usages(limit)
            .await
            .map_err(internal)?,
    ))
}

/// The `?granularity=&from=&to=` selector for rollups.
#[derive(Debug, Clone, Deserialize)]
pub struct RollupQuery {
    pub granularity: String,
    pub from: i64,
    pub to: i64,
}

/// `GET /admin/usage-rollups?granularity=hour|day|week|month&from=&to=` —
/// rollup buckets for one granularity in `[from, to]`.
pub async fn list_usage_rollups(
    State(state): State<AppState>,
    Query(q): Query<RollupQuery>,
) -> Result<Json<Vec<UsageRollup>>, ApiError> {
    if !matches!(q.granularity.as_str(), "hour" | "day" | "week" | "month") {
        return Err(ApiError::BadRequest(
            "granularity must be one of hour|day|week|month".into(),
        ));
    }
    Ok(Json(
        state
            .persistence
            .list_usage_rollups(&q.granularity, q.from, q.to)
            .await
            .map_err(internal)?,
    ))
}

/// `GET /admin/audit?limit=N` — the most recent audit rows (id desc). Audit
/// rows carry only method/path/actor/status/ip; never a secret.
pub async fn list_audit(
    State(state): State<AppState>,
    Query(q): Query<UsageQuery>,
) -> Result<Json<Vec<AuditLog>>, ApiError> {
    let limit = q.limit.unwrap_or(DEFAULT_USAGE_LIMIT).min(MAX_USAGE_LIMIT);
    Ok(Json(
        state
            .persistence
            .list_audit_logs(limit)
            .await
            .map_err(internal)?,
    ))
}

/// `GET /admin/credentials/{id}/status` — the persisted credential health
/// snapshots (§16.3 edge-triggered breaker/cooldown rows) for one credential.
pub async fn credential_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<CredentialStatus>>, ApiError> {
    Ok(Json(
        state
            .persistence
            .list_credential_statuses(id)
            .await
            .map_err(internal)?,
    ))
}

/// `GET /admin/logs/{request_id}/downstream` — downstream (client → proxy) log
/// entries correlated by `request_id` (§15).
pub async fn downstream_logs(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<Json<Vec<DownstreamRequest>>, ApiError> {
    Ok(Json(
        state
            .persistence
            .list_downstream_requests(&request_id)
            .await
            .map_err(internal)?,
    ))
}

/// `GET /admin/logs/{request_id}/upstream` — upstream (proxy → provider) log
/// entries correlated by `request_id` (§15).
pub async fn upstream_logs(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<Json<Vec<UpstreamRequest>>, ApiError> {
    Ok(Json(
        state
            .persistence
            .list_upstream_requests(&request_id)
            .await
            .map_err(internal)?,
    ))
}
