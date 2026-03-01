use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::AppState;

use super::error::HttpError;

#[derive(Debug, Deserialize)]
pub struct LoginPayload {
    pub name: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user_id: i64,
    pub api_key: String,
    pub generated: bool,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LoginPayload>,
) -> Result<Json<LoginResponse>, HttpError> {
    let users = state.load_users();
    let user = gproxy_admin::authenticate_user_password(
        &users,
        payload.name.as_str(),
        payload.password.as_str(),
    )?;

    let keys = state.load_keys();
    if let Some(existing) = keys
        .values()
        .filter(|row| row.user_id == user.id && row.enabled)
        .min_by_key(|row| row.id)
    {
        return Ok(Json(LoginResponse {
            user_id: user.id,
            api_key: existing.api_key.clone(),
            generated: false,
        }));
    }

    let api_key = gproxy_admin::generate_unique_user_api_key(&keys).map_err(HttpError::from)?;

    let next_id = keys.values().map(|row| row.id).max().unwrap_or(-1) + 1;
    let write = gproxy_storage::UserKeyWrite {
        id: next_id,
        user_id: user.id,
        api_key: api_key.clone(),
        label: Some("auto-generated".to_string()),
        enabled: true,
    };
    state
        .storage_writes
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertUserKey(
            write.clone(),
        ))
        .await
        .map_err(gproxy_admin::AdminApiError::from)
        .map_err(HttpError::from)?;
    state.upsert_user_key_in_memory(write);

    Ok(Json(LoginResponse {
        user_id: user.id,
        api_key,
        generated: true,
    }))
}
