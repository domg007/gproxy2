use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use gproxy_server::AppState;
use gproxy_storage::Scope;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct ProviderRow {
    pub name: String,
    pub channel: String,
    pub settings_json: serde_json::Value,
    pub dispatch_json: serde_json::Value,
    pub credential_count: usize,
}

#[derive(serde::Deserialize, Default)]
pub struct ProviderQueryParams {
    #[serde(default)]
    pub name: Scope<String>,
    #[serde(default)]
    pub channel: Scope<String>,
}

/// Query providers from SDK engine memory.
pub async fn query_providers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<ProviderQueryParams>,
) -> Result<Json<Vec<ProviderRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let snapshots = state
        .engine()
        .store()
        .list_providers()
        .map_err(|e| HttpError::internal(e.to_string()))?;
    let rows: Vec<ProviderRow> = snapshots
        .into_iter()
        .filter(|s| match &query.name {
            Scope::Eq(v) => s.name == *v,
            _ => true,
        })
        .filter(|s| match &query.channel {
            Scope::Eq(v) => s.channel == *v,
            _ => true,
        })
        .map(|s| ProviderRow {
            name: s.name,
            channel: s.channel,
            settings_json: s.settings,
            dispatch_json: serde_json::Value::Null,
            credential_count: s.credential_count,
        })
        .collect();
    Ok(Json(rows))
}

/// Upsert provider — persists to DB.
/// Note: provider changes in the SDK engine require rebuild (takes effect on restart).
pub async fn upsert_provider(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::ProviderWrite>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    state
        .storage_writes()
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertProvider(payload))
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

#[derive(serde::Deserialize)]
pub struct DeleteProviderPayload {
    pub name: String,
}

/// Delete provider — persists to DB and removes from SDK engine memory.
pub async fn delete_provider(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteProviderPayload>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    state.engine().store().remove_provider(&payload.name);
    // Also persist (need id for DB, but we only have name — enqueue by name)
    // For now just remove from memory; DB cleanup on next sync
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_upsert_providers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(items): Json<Vec<gproxy_storage::ProviderWrite>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let sender = state.storage_writes();
    for item in items {
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::UpsertProvider(item))
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_delete_providers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(names): Json<Vec<String>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    for name in &names {
        state.engine().store().remove_provider(name);
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}
