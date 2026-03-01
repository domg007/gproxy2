use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;

use crate::AppState;

use super::{Ack, DeleteById, HttpError, UpsertUserPayload, authorize_admin};

pub(super) async fn query_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UserQuery>,
) -> Result<Json<Vec<gproxy_storage::UserQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let users = state.load_users();
    Ok(Json(gproxy_admin::query_users(&users, query).await?))
}

pub(super) async fn upsert_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UpsertUserPayload>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(HttpError::from(gproxy_admin::AdminApiError::InvalidInput(
            "user name cannot be empty".to_string(),
        )));
    }
    let id = payload.id.unwrap_or_else(|| {
        state
            .load_users()
            .iter()
            .map(|row| row.id)
            .max()
            .unwrap_or(-1)
            + 1
    });
    let write = gproxy_storage::UserWrite {
        id,
        name: name.to_string(),
        enabled: payload.enabled,
    };
    state.upsert_user_in_memory(write.clone());
    gproxy_admin::upsert_user(&state.storage_writes, write).await?;
    Ok(Json(Ack { ok: true }))
}

pub(super) async fn delete_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteById>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    state.delete_user_in_memory(payload.id);
    gproxy_admin::delete_user(&state.storage_writes, payload.id).await?;
    Ok(Json(Ack { ok: true }))
}
