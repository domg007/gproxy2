use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;

use crate::AppState;

use super::{Ack, DeleteById, HttpError, authorize_admin};

pub(super) async fn query_user_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UserKeyQuery>,
) -> Result<Json<Vec<gproxy_storage::UserKeyQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let keys = state.load_keys();
    Ok(Json(gproxy_admin::query_user_keys(&keys, query).await?))
}

pub(super) async fn upsert_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::UserKeyWrite>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    let keys = state.load_keys();
    let row = gproxy_admin::upsert_user_key(&keys, &state.storage_writes, payload).await?;
    state.upsert_user_key_in_memory(row);
    Ok(Json(Ack { ok: true }))
}

pub(super) async fn delete_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteById>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    state.delete_user_key_in_memory(payload.id);
    gproxy_admin::delete_user_key(&state.storage_writes, payload.id).await?;
    Ok(Json(Ack { ok: true }))
}
