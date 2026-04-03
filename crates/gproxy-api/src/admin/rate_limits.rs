use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use gproxy_server::{AppState, RateLimitRule};
use gproxy_storage::Scope;
use std::sync::Arc;

/// Response row for rate limits from memory (no timestamps or row id).
#[derive(serde::Serialize)]
pub struct MemoryRateLimitRow {
    pub user_id: i64,
    pub model_pattern: String,
    pub rpm: Option<i32>,
    pub rpd: Option<i32>,
    pub total_tokens: Option<i64>,
}

/// Query filter for rate limits.
#[derive(serde::Deserialize, Default)]
pub struct RateLimitQueryParams {
    pub user_id: Option<Scope<i64>>,
    pub limit: Option<usize>,
}

pub async fn query_rate_limits(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<RateLimitQueryParams>,
) -> Result<Json<Vec<MemoryRateLimitRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let limits = state.user_rate_limits_snapshot();
    let mut rows: Vec<MemoryRateLimitRow> = Vec::new();
    for (&user_id, rules) in limits.iter() {
        match &query.user_id {
            Some(Scope::Eq(v)) if *v != user_id => continue,
            Some(Scope::In(vs)) if !vs.contains(&user_id) => continue,
            _ => {}
        }
        for rule in rules {
            rows.push(MemoryRateLimitRow {
                user_id,
                model_pattern: rule.model_pattern.clone(),
                rpm: rule.rpm,
                rpd: rule.rpd,
                total_tokens: rule.total_tokens,
            });
        }
    }
    if let Some(limit) = query.limit {
        rows.truncate(limit);
    }
    Ok(Json(rows))
}

pub async fn upsert_rate_limit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::UserRateLimitWrite>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    // Sync in-memory state
    state.upsert_rate_limit_in_memory(
        payload.user_id,
        RateLimitRule {
            model_pattern: payload.model_pattern.clone(),
            rpm: payload.rpm,
            rpd: payload.rpd,
            total_tokens: payload.total_tokens,
        },
    );

    // Enqueue DB write
    let sender = state.storage_writes();
    sender
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertUserRateLimit(
            payload,
        ))
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

#[derive(serde::Deserialize)]
pub struct DeleteRateLimitPayload {
    pub id: i64,
    pub user_id: i64,
    pub model_pattern: String,
}

pub async fn delete_rate_limit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteRateLimitPayload>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    // Sync in-memory state
    state.remove_rate_limit_from_memory(payload.user_id, &payload.model_pattern);

    // Enqueue DB write
    let sender = state.storage_writes();
    sender
        .enqueue(gproxy_storage::StorageWriteEvent::DeleteUserRateLimit { id: payload.id })
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_upsert_rate_limits(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(items): Json<Vec<gproxy_storage::UserRateLimitWrite>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let sender = state.storage_writes();
    for item in items {
        state.upsert_rate_limit_in_memory(
            item.user_id,
            RateLimitRule {
                model_pattern: item.model_pattern.clone(),
                rpm: item.rpm,
                rpd: item.rpd,
                total_tokens: item.total_tokens,
            },
        );
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::UpsertUserRateLimit(item))
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_delete_rate_limits(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payloads): Json<Vec<DeleteRateLimitPayload>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let sender = state.storage_writes();
    for p in payloads {
        state.remove_rate_limit_from_memory(p.user_id, &p.model_pattern);
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::DeleteUserRateLimit { id: p.id })
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}
