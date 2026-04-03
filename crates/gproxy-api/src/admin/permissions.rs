use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use gproxy_server::{AppState, PermissionEntry};
use gproxy_storage::Scope;
use std::sync::Arc;

/// Response row for permissions from memory (no timestamps or row id).
#[derive(serde::Serialize)]
pub struct MemoryPermissionRow {
    pub user_id: i64,
    pub provider_id: Option<i64>,
    pub model_pattern: String,
}

/// Query filter for permissions.
#[derive(serde::Deserialize, Default)]
pub struct PermissionQueryParams {
    pub user_id: Option<Scope<i64>>,
    pub provider_id: Option<Scope<i64>>,
    pub limit: Option<usize>,
}

pub async fn query_permissions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<PermissionQueryParams>,
) -> Result<Json<Vec<MemoryPermissionRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let perms = state.user_permissions_snapshot();
    let mut rows: Vec<MemoryPermissionRow> = Vec::new();
    for (&user_id, entries) in perms.iter() {
        // Filter by user_id
        match &query.user_id {
            Some(Scope::Eq(v)) if *v != user_id => continue,
            Some(Scope::In(vs)) if !vs.contains(&user_id) => continue,
            _ => {}
        }
        for entry in entries {
            // Filter by provider_id
            match (&query.provider_id, &entry.provider_id) {
                (Some(Scope::Eq(v)), Some(pid)) if v != pid => continue,
                (Some(Scope::Eq(_)), None) => continue,
                (Some(Scope::In(vs)), Some(pid)) if !vs.contains(pid) => continue,
                (Some(Scope::In(_)), None) => continue,
                _ => {}
            }
            rows.push(MemoryPermissionRow {
                user_id,
                provider_id: entry.provider_id,
                model_pattern: entry.model_pattern.clone(),
            });
        }
    }
    if let Some(limit) = query.limit {
        rows.truncate(limit);
    }
    Ok(Json(rows))
}

pub async fn upsert_permission(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::UserModelPermissionWrite>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    // Sync in-memory state
    state.upsert_permission_in_memory(
        payload.user_id,
        PermissionEntry {
            provider_id: payload.provider_id,
            model_pattern: payload.model_pattern.clone(),
        },
    );

    // Enqueue DB write
    let sender = state.storage_writes();
    sender
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertUserModelPermission(payload))
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

#[derive(serde::Deserialize)]
pub struct DeletePermissionPayload {
    pub id: i64,
    pub user_id: i64,
    pub provider_id: Option<i64>,
    pub model_pattern: String,
}

pub async fn delete_permission(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeletePermissionPayload>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    // Sync in-memory state
    state.remove_permission_from_memory(
        payload.user_id,
        payload.provider_id,
        &payload.model_pattern,
    );

    // Enqueue DB write
    let sender = state.storage_writes();
    sender
        .enqueue(gproxy_storage::StorageWriteEvent::DeleteUserModelPermission { id: payload.id })
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_upsert_permissions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(items): Json<Vec<gproxy_storage::UserModelPermissionWrite>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let sender = state.storage_writes();
    for item in items {
        state.upsert_permission_in_memory(
            item.user_id,
            PermissionEntry {
                provider_id: item.provider_id,
                model_pattern: item.model_pattern.clone(),
            },
        );
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::UpsertUserModelPermission(item))
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_delete_permissions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payloads): Json<Vec<DeletePermissionPayload>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let sender = state.storage_writes();
    for p in payloads {
        state.remove_permission_from_memory(p.user_id, p.provider_id, &p.model_pattern);
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::DeleteUserModelPermission { id: p.id })
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}
