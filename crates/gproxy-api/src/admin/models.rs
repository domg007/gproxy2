use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use gproxy_server::{AppState, MemoryModel, ModelAliasTarget};
use gproxy_storage::Scope;
use std::sync::Arc;

/// Response row for query_models (from in-memory data, no timestamps).
#[derive(serde::Serialize)]
pub struct MemoryModelRow {
    pub id: i64,
    pub provider_id: i64,
    pub model_id: String,
    pub display_name: Option<String>,
    pub enabled: bool,
    pub price_each_call: Option<f64>,
    pub price_input_tokens: Option<f64>,
    pub price_output_tokens: Option<f64>,
    pub price_cache_read_input_tokens: Option<f64>,
    pub price_cache_creation_input_tokens: Option<f64>,
    pub price_cache_creation_input_tokens_5min: Option<f64>,
    pub price_cache_creation_input_tokens_1h: Option<f64>,
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
            price_input_tokens: m.price_input_tokens,
            price_output_tokens: m.price_output_tokens,
            price_cache_read_input_tokens: m.price_cache_read_input_tokens,
            price_cache_creation_input_tokens: m.price_cache_creation_input_tokens,
            price_cache_creation_input_tokens_5min: m.price_cache_creation_input_tokens_5min,
            price_cache_creation_input_tokens_1h: m.price_cache_creation_input_tokens_1h,
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
    state.upsert_model_in_memory(MemoryModel {
        id: payload.id,
        provider_id: payload.provider_id,
        model_id: payload.model_id.clone(),
        display_name: payload.display_name.clone(),
        enabled: payload.enabled,
        price_each_call: payload.price_each_call,
        price_input_tokens: payload.price_input_tokens,
        price_output_tokens: payload.price_output_tokens,
        price_cache_read_input_tokens: payload.price_cache_read_input_tokens,
        price_cache_creation_input_tokens: payload.price_cache_creation_input_tokens,
        price_cache_creation_input_tokens_5min: payload.price_cache_creation_input_tokens_5min,
        price_cache_creation_input_tokens_1h: payload.price_cache_creation_input_tokens_1h,
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

    // Sync in-memory state (use provider_id.to_string() as provider_name for now)
    state.upsert_model_alias_in_memory(
        payload.alias.clone(),
        ModelAliasTarget {
            provider_name: payload.provider_id.to_string(),
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
        state.upsert_model_in_memory(MemoryModel {
            id: item.id,
            provider_id: item.provider_id,
            model_id: item.model_id.clone(),
            display_name: item.display_name.clone(),
            enabled: item.enabled,
            price_each_call: item.price_each_call,
            price_input_tokens: item.price_input_tokens,
            price_output_tokens: item.price_output_tokens,
            price_cache_read_input_tokens: item.price_cache_read_input_tokens,
            price_cache_creation_input_tokens: item.price_cache_creation_input_tokens,
            price_cache_creation_input_tokens_5min: item.price_cache_creation_input_tokens_5min,
            price_cache_creation_input_tokens_1h: item.price_cache_creation_input_tokens_1h,
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
    let sender = state.storage_writes();
    for item in items {
        state.upsert_model_alias_in_memory(
            item.alias.clone(),
            ModelAliasTarget {
                provider_name: item.provider_id.to_string(),
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
