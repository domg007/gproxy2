use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use gproxy_server::AppState;
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
    State(_state): State<Arc<AppState>>,
    Json(_payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, HttpError> {
    // TODO: validate username/password against storage, return API key
    Err(HttpError::internal("login not yet implemented"))
}
