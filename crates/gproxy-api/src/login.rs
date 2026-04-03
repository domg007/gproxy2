use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use gproxy_server::AppState;
use gproxy_storage::Scope;

use crate::error::HttpError;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub user_id: i64,
    pub api_key: String,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, HttpError> {
    let storage = state.storage();
    let users = storage
        .list_users(&gproxy_storage::UserQuery {
            name: Scope::Eq(payload.username.clone()),
            ..Default::default()
        })
        .await?;

    let user = users
        .first()
        .ok_or_else(|| HttpError::unauthorized("invalid username or password"))?;

    if user.password != payload.password {
        return Err(HttpError::unauthorized("invalid username or password"));
    }

    if !user.enabled {
        return Err(HttpError::forbidden("user is disabled"));
    }

    let keys = storage
        .list_user_keys(&gproxy_storage::UserKeyQuery {
            user_id: Scope::Eq(user.id),
            ..Default::default()
        })
        .await?;

    let key = keys
        .first()
        .ok_or_else(|| HttpError::internal("user has no API key"))?;

    Ok(Json(LoginResponse {
        user_id: user.id,
        api_key: key.api_key.clone(),
    }))
}
