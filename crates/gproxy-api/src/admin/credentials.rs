use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use gproxy_server::AppState;
use gproxy_storage::{ProviderQuery, ProviderQueryRow, Scope};
use serde::Serialize;
use std::sync::Arc;

/// Look up a provider row from the DB by name.
async fn resolve_provider_by_name(
    state: &AppState,
    provider_name: &str,
) -> Result<ProviderQueryRow, HttpError> {
    let rows = state
        .storage()
        .list_providers(&ProviderQuery {
            name: Scope::Eq(provider_name.to_string()),
            ..Default::default()
        })
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    rows.into_iter()
        .next()
        .ok_or_else(|| HttpError::not_found(format!("provider '{provider_name}' not found")))
}

/// Look up the DB id of a credential given its provider_id and positional index.
async fn resolve_credential_db_id(
    state: &AppState,
    provider_id: i64,
    index: usize,
) -> Result<i64, HttpError> {
    let creds = state
        .storage()
        .list_credentials(&gproxy_storage::CredentialQuery {
            provider_id: Scope::Eq(provider_id),
            ..Default::default()
        })
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    creds
        .get(index)
        .map(|c| c.id)
        .ok_or_else(|| HttpError::not_found("credential index out of range"))
}

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

/// Generate a unique ID for new credentials using timestamp + random bits.
fn generate_credential_id() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let random: u16 = rand::random();
    ts * 1000 + (random % 1000) as i64
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
    let provider = resolve_provider_by_name(&state, &payload.provider_name).await?;
    let write = gproxy_storage::CredentialWrite {
        id: generate_credential_id(),
        provider_id: provider.id,
        name: None,
        kind: provider.channel.clone(),
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
    // Persist deletion to DB
    let provider = resolve_provider_by_name(&state, &payload.provider_name).await?;
    let cred_id = resolve_credential_db_id(&state, provider.id, payload.index).await?;
    state
        .storage_writes()
        .enqueue(gproxy_storage::StorageWriteEvent::DeleteCredential { id: cred_id })
        .await
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
    let sender = state.storage_writes();
    for item in &items {
        store
            .add_credential(&item.provider_name, item.credential.clone())
            .map_err(|e| HttpError::internal(e.to_string()))?;
        // Persist to DB
        let provider = resolve_provider_by_name(&state, &item.provider_name).await?;
        let write = gproxy_storage::CredentialWrite {
            id: generate_credential_id(),
            provider_id: provider.id,
            name: None,
            kind: provider.channel.clone(),
            secret_json: item.credential.to_string(),
            enabled: true,
        };
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::UpsertCredential(write))
            .await
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
    let sender = state.storage_writes();
    // Delete in reverse index order to avoid index shifting
    let mut sorted = items;
    sorted.sort_by(|a, b| b.index.cmp(&a.index));
    for item in &sorted {
        // Resolve DB id before removing from memory (index is still valid)
        let provider = resolve_provider_by_name(&state, &item.provider_name).await?;
        let cred_id = resolve_credential_db_id(&state, provider.id, item.index).await?;
        let _ = store.remove_credential(&item.provider_name, item.index);
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::DeleteCredential { id: cred_id })
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?;
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
