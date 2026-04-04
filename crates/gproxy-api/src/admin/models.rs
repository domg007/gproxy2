use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use gproxy_server::{AppState, MemoryModel, ModelAliasTarget, PriceTier};
use gproxy_storage::Scope;
use std::collections::HashMap;
use std::sync::Arc;

/// Resolve a single provider_id to its name via storage query.
async fn resolve_provider_name(state: &AppState, provider_id: i64) -> Result<String, HttpError> {
    let storage = state.storage();
    let providers = storage
        .list_providers(&gproxy_storage::ProviderQuery::default())
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    providers
        .iter()
        .find(|p| p.id == provider_id)
        .map(|p| p.name.clone())
        .ok_or_else(|| HttpError::internal(format!("provider_id {} not found", provider_id)))
}

/// Build a provider_id -> name map for a set of provider IDs.
async fn resolve_provider_names(state: &AppState) -> Result<HashMap<i64, String>, HttpError> {
    let storage = state.storage();
    let providers = storage
        .list_providers(&gproxy_storage::ProviderQuery::default())
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(providers.into_iter().map(|p| (p.id, p.name)).collect())
}

/// Response row for query_models (from in-memory data, no timestamps).
#[derive(serde::Serialize)]
pub struct MemoryModelRow {
    pub id: i64,
    pub provider_id: i64,
    pub model_id: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub price_each_call: Option<f64>,
    pub price_tiers: Vec<PriceTier>,
}

/// Query filter for models (simplified from storage ModelQuery).
#[derive(serde::Deserialize, Default)]
pub struct ModelQueryParams {
    pub id: Option<Scope<i64>>,
    pub provider_id: Option<Scope<i64>>,
    pub model_id: Option<Scope<String>>,
    pub enabled: Option<Scope<bool>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

fn scope_matches<T: PartialEq>(scope: &Option<Scope<T>>, value: &T) -> bool {
    match scope {
        None => true,
        Some(Scope::All) => true,
        Some(Scope::Eq(v)) => v == value,
        Some(Scope::In(vs)) => vs.contains(value),
    }
}

pub async fn query_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<ModelQueryParams>,
) -> Result<Json<Vec<MemoryModelRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let models = state.models();
    let mut rows: Vec<MemoryModelRow> = models
        .iter()
        .filter(|m| {
            scope_matches(&query.id, &m.id)
                && scope_matches(&query.provider_id, &m.provider_id)
                && scope_matches(&query.model_id, &m.model_id)
                && scope_matches(&query.enabled, &m.enabled)
        })
        .map(|m| MemoryModelRow {
            id: m.id,
            provider_id: m.provider_id,
            model_id: m.model_id.clone(),
            display_name: m.display_name.clone(),
            enabled: m.enabled,
            price_each_call: m.price_each_call,
            price_tiers: m.price_tiers.clone(),
        })
        .collect();

    let offset = query.offset.unwrap_or(0);
    if offset > 0 {
        rows = rows.into_iter().skip(offset).collect();
    }
    if let Some(limit) = query.limit {
        rows.truncate(limit);
    }
    Ok(Json(rows))
}

pub async fn upsert_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::ModelWrite>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    // Sync in-memory state
    let price_tiers: Vec<PriceTier> = payload
        .price_tiers_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    state.upsert_model_in_memory(MemoryModel {
        id: payload.id,
        provider_id: payload.provider_id,
        model_id: payload.model_id.clone(),
        display_name: payload.display_name.clone(),
        enabled: payload.enabled,
        price_each_call: payload.price_each_call,
        price_tiers,
    });

    // Enqueue DB write
    let sender = state.storage_writes();
    sender
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertModel(payload))
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

#[derive(serde::Deserialize)]
pub struct DeleteModelPayload {
    id: i64,
}

pub async fn delete_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteModelPayload>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    // Sync in-memory state
    state.remove_model_from_memory(payload.id);

    // Enqueue DB write
    let sender = state.storage_writes();
    sender
        .enqueue(gproxy_storage::StorageWriteEvent::DeleteModel { id: payload.id })
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

/// Response row for model aliases from memory.
#[derive(serde::Serialize)]
pub struct MemoryModelAliasRow {
    pub alias: String,
    pub provider_name: String,
    pub model_id: String,
}

pub async fn query_model_aliases(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<MemoryModelAliasRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let aliases = state.model_aliases_snapshot();
    let rows: Vec<MemoryModelAliasRow> = aliases
        .iter()
        .map(|(alias, target)| MemoryModelAliasRow {
            alias: alias.clone(),
            provider_name: target.provider_name.clone(),
            model_id: target.model_id.clone(),
        })
        .collect();
    Ok(Json(rows))
}

pub async fn upsert_model_alias(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::ModelAliasWrite>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    // Resolve provider_id → provider_name from storage
    let provider_name = resolve_provider_name(&state, payload.provider_id).await?;

    // Sync in-memory state
    state.upsert_model_alias_in_memory(
        payload.alias.clone(),
        ModelAliasTarget {
            provider_name,
            model_id: payload.model_id.clone(),
        },
    );

    // Enqueue DB write
    let sender = state.storage_writes();
    sender
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertModelAlias(payload))
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

#[derive(serde::Deserialize)]
pub struct DeleteModelAliasPayload {
    id: i64,
    alias: String,
}

pub async fn delete_model_alias(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteModelAliasPayload>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    // Sync in-memory state
    state.remove_model_alias_from_memory(&payload.alias);

    // Enqueue DB write
    let sender = state.storage_writes();
    sender
        .enqueue(gproxy_storage::StorageWriteEvent::DeleteModelAlias { id: payload.id })
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_upsert_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(items): Json<Vec<gproxy_storage::ModelWrite>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let sender = state.storage_writes();
    for item in items {
        let price_tiers: Vec<PriceTier> = item
            .price_tiers_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        state.upsert_model_in_memory(MemoryModel {
            id: item.id,
            provider_id: item.provider_id,
            model_id: item.model_id.clone(),
            display_name: item.display_name.clone(),
            enabled: item.enabled,
            price_each_call: item.price_each_call,
            price_tiers,
        });
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::UpsertModel(item))
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_delete_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(ids): Json<Vec<i64>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let sender = state.storage_writes();
    for id in ids {
        state.remove_model_from_memory(id);
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::DeleteModel { id })
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_upsert_model_aliases(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(items): Json<Vec<gproxy_storage::ModelAliasWrite>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    // Build provider_id -> name map for all referenced providers
    let provider_name_map = resolve_provider_names(&state).await?;

    let sender = state.storage_writes();
    for item in items {
        let provider_name = provider_name_map
            .get(&item.provider_id)
            .cloned()
            .unwrap_or_else(|| item.provider_id.to_string());
        state.upsert_model_alias_in_memory(
            item.alias.clone(),
            ModelAliasTarget {
                provider_name,
                model_id: item.model_id.clone(),
            },
        );
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::UpsertModelAlias(item))
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}

pub async fn batch_delete_model_aliases(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payloads): Json<Vec<DeleteModelAliasPayload>>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let sender = state.storage_writes();
    for p in payloads {
        state.remove_model_alias_from_memory(&p.alias);
        sender
            .enqueue(gproxy_storage::StorageWriteEvent::DeleteModelAlias { id: p.id })
            .await
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    Ok(Json(AckResponse { ok: true, id: None }))
}
