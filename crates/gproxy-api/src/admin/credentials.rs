use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use gproxy_server::AppState;
use gproxy_storage::Scope;
use serde::Serialize;
use std::sync::Arc;

#[derive(serde::Deserialize, Default)]
pub struct CredentialQueryParams {
    #[serde(default)]
    pub provider_name: Scope<String>,
}

#[derive(Serialize)]
pub struct CredentialRow {
    pub provider: String,
    pub index: usize,
    pub credential: serde_json::Value,
}

/// Query credentials from SDK engine memory.
pub async fn query_credentials(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<CredentialQueryParams>,
) -> Result<Json<Vec<CredentialRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let provider_name = match &query.provider_name {
        Scope::Eq(v) => Some(v.as_str()),
        _ => None,
    };
    let creds = state
        .engine()
        .store()
        .list_credentials(provider_name)
        .map_err(|e| HttpError::internal(e.to_string()))?;
    let rows = creds
        .into_iter()
        .map(|c| CredentialRow {
            provider: c.provider,
            index: c.index,
            credential: c.credential,
        })
        .collect();
    Ok(Json(rows))
}

#[derive(serde::Deserialize)]
pub struct UpsertCredentialPayload {
    pub provider_name: String,
    pub credential: serde_json::Value,
}

/// Add or update a credential in SDK engine memory + persist to DB.
pub async fn upsert_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UpsertCredentialPayload>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    // Add to SDK engine memory
    state
        .engine()
        .store()
        .add_credential(&payload.provider_name, payload.credential.clone())
        .map_err(|e| HttpError::internal(e.to_string()))?;
    // Persist to DB
    let write = gproxy_storage::CredentialWrite {
        id: 0,          // auto-assign
        provider_id: 0, // TODO: resolve from provider name
        name: None,
        kind: String::new(),
        secret_json: payload.credential.to_string(),
        enabled: true,
    };
    state
        .storage_writes()
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertCredential(write))
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

#[derive(serde::Deserialize)]
pub struct DeleteCredentialPayload {
    pub provider_name: String,
    pub index: usize,
}

/// Remove a credential from SDK engine memory + persist to DB.
pub async fn delete_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteCredentialPayload>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    state
        .engine()
        .store()
        .remove_credential(&payload.provider_name, payload.index)
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_upsert_credentials(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(items): Json<Vec<UpsertCredentialPayload>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let engine = state.engine();
    let store = engine.store();
    for item in &items {
        store
            .add_credential(&item.provider_name, item.credential.clone())
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_delete_credentials(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(items): Json<Vec<DeleteCredentialPayload>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let engine = state.engine();
    let store = engine.store();
    // Delete in reverse index order to avoid index shifting
    let mut sorted = items;
    sorted.sort_by(|a, b| b.index.cmp(&a.index));
    for item in &sorted {
        let _ = store.remove_credential(&item.provider_name, item.index);
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}

#[derive(serde::Deserialize, Default)]
pub struct HealthQueryParams {
    #[serde(default)]
    pub provider_name: Scope<String>,
}

/// Query credential health from SDK engine memory.
pub async fn query_credential_statuses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<HealthQueryParams>,
) -> Result<Json<Vec<gproxy_sdk::provider::store::CredentialHealthSnapshot>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let provider_name = match &query.provider_name {
        Scope::Eq(v) => Some(v.as_str()),
        _ => None,
    };
    let snapshots = state.engine().store().list_health(provider_name);
    Ok(Json(snapshots))
}

#[derive(serde::Deserialize)]
pub struct UpdateCredentialStatusPayload {
    pub provider_name: String,
    pub index: usize,
    /// `"healthy"` or `"dead"`.
    pub status: String,
}

/// Manually set credential health status (admin override).
pub async fn update_credential_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UpdateCredentialStatusPayload>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let engine = state.engine();
    let store = engine.store();
    let ok = match payload.status.as_str() {
        "dead" => store.mark_credential_dead(&payload.provider_name, payload.index),
        "healthy" => store.mark_credential_healthy(&payload.provider_name, payload.index),
        _ => {
            return Err(HttpError::bad_request("status must be 'healthy' or 'dead'"));
        }
    };
    if !ok {
        return Err(HttpError::not_found(
            "provider or credential index not found",
        ));
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}
