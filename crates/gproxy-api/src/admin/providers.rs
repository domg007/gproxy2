use crate::auth::authorize_admin;
use crate::bootstrap::apply_persisted_credential_statuses;
use crate::error::{AckResponse, HttpError};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use gproxy_sdk::provider::engine::ProviderConfig;
use gproxy_server::AppState;
use gproxy_storage::{CredentialQuery, ProviderQuery, ProviderQueryRow, Scope};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Look up a provider's DB id by name.
async fn resolve_provider_id_by_name(state: &AppState, name: &str) -> Result<i64, HttpError> {
    let rows = state
        .storage()
        .list_providers(&ProviderQuery {
            name: Scope::Eq(name.to_string()),
            ..Default::default()
        })
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    rows.into_iter()
        .next()
        .map(|r| r.id)
        .ok_or_else(|| HttpError::not_found(format!("provider '{name}' not found in DB")))
}

async fn load_providers_by_id(
    state: &AppState,
) -> Result<HashMap<i64, ProviderQueryRow>, HttpError> {
    let rows = state
        .storage()
        .list_providers(&ProviderQuery::default())
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(rows.into_iter().map(|row| (row.id, row)).collect())
}

async fn sync_provider_runtime(
    state: &AppState,
    payload: &gproxy_storage::ProviderWrite,
    previous_name: Option<&str>,
) -> Result<(), HttpError> {
    let store = state.engine().store().clone();
    let previous_runtime_name = if let Some(old_name) = previous_name {
        if store
            .get_provider(old_name)
            .map_err(|e| HttpError::internal(e.to_string()))?
            .is_some()
        {
            Some(old_name.to_string())
        } else {
            None
        }
    } else {
        None
    };

    if let Some(old_name) = previous_name
        && old_name != payload.name
    {
        store.remove_provider(old_name);
        state.remove_provider_name_from_memory(old_name);
        state.remove_provider_channel_from_memory(old_name);
        state.remove_provider_credentials_from_memory(old_name);
    }

    state.upsert_provider_name_in_memory(payload.name.clone(), payload.id);
    state.upsert_provider_channel_in_memory(payload.name.clone(), payload.channel.clone());

    let settings_json = serde_json::from_str(&payload.settings_json).unwrap_or_default();
    let current = store
        .get_provider(&payload.name)
        .map_err(|e| HttpError::internal(e.to_string()))?;

    if let Some(snapshot) = current
        && snapshot.channel == payload.channel
    {
        store
            .update_provider_settings(&payload.name, settings_json)
            .map_err(|e| HttpError::internal(e.to_string()))?;
        return Ok(());
    }

    store.remove_provider(&payload.name);

    let credentials = state
        .storage()
        .list_credentials(&CredentialQuery {
            provider_id: Scope::Eq(payload.id),
            enabled: Scope::Eq(true),
            ..Default::default()
        })
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    let runtime_credentials = previous_runtime_name
        .as_deref()
        .or(Some(payload.name.as_str()))
        .and_then(|provider_name| store.list_credentials(Some(provider_name)).ok())
        .filter(|creds| !creds.is_empty())
        .map(|creds| {
            creds
                .into_iter()
                .map(|cred| cred.credential)
                .collect::<Vec<_>>()
        });
    let credential_ids = if runtime_credentials.is_some() {
        state
            .provider_credential_ids_for(previous_runtime_name.as_deref().unwrap_or(&payload.name))
            .unwrap_or_else(|| credentials.iter().map(|cred| cred.id).collect())
    } else {
        credentials.iter().map(|cred| cred.id).collect()
    };

    let provider_config = ProviderConfig {
        name: payload.name.clone(),
        channel: payload.channel.clone(),
        settings_json: serde_json::from_str(&payload.settings_json).unwrap_or_default(),
        credentials: runtime_credentials.unwrap_or_else(|| {
            credentials
                .iter()
                .map(|cred| cred.secret_json.clone())
                .collect()
        }),
    };
    store
        .add_provider_json(provider_config)
        .map_err(|e| HttpError::internal(e.to_string()))?;
    state.replace_provider_credential_ids_in_memory(payload.name.clone(), credential_ids);

    let credential_positions: HashMap<i64, (String, usize)> = credentials
        .iter()
        .enumerate()
        .map(|(index, cred)| (cred.id, (payload.name.clone(), index)))
        .collect();
    apply_persisted_credential_statuses(state, &credential_positions)
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;

    Ok(())
}

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
    let store = state.engine().store().clone();
    let snapshots = store
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
        .map(|s| {
            let dispatch_json = store
                .get_dispatch_table(&s.name)
                .and_then(|dt| serde_json::to_value(&dt).ok())
                .unwrap_or(serde_json::Value::Null);
            ProviderRow {
                name: s.name,
                channel: s.channel,
                settings_json: s.settings,
                dispatch_json,
                credential_count: s.credential_count,
            }
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
    let previous_name = load_providers_by_id(&state)
        .await?
        .get(&payload.id)
        .map(|row| row.name.clone());
    sync_provider_runtime(&state, &payload, previous_name.as_deref()).await?;
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
    let provider_id = resolve_provider_id_by_name(&state, &payload.name).await?;
    state.engine().store().remove_provider(&payload.name);
    state.remove_provider_name_from_memory(&payload.name);
    state.remove_provider_channel_from_memory(&payload.name);
    state.remove_provider_credentials_from_memory(&payload.name);
    state.remove_user_files_for_provider(provider_id);
    state
        .storage_writes()
        .enqueue(gproxy_storage::StorageWriteEvent::DeleteProvider { id: provider_id })
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_upsert_providers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(items): Json<Vec<gproxy_storage::ProviderWrite>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let existing = load_providers_by_id(&state).await?;
    let sender = state.storage_writes();
    for item in items {
        let previous_name = existing.get(&item.id).map(|row| row.name.as_str());
        sync_provider_runtime(&state, &item, previous_name).await?;
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
    let sender = state.storage_writes();
    for name in &names {
        let provider_id = resolve_provider_id_by_name(&state, name).await?;
        state.engine().store().remove_provider(name);
        state.remove_provider_name_from_memory(name);
        state.remove_provider_channel_from_memory(name);
        state.remove_provider_credentials_from_memory(name);
        state.remove_user_files_for_provider(provider_id);
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::DeleteProvider { id: provider_id })
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}
