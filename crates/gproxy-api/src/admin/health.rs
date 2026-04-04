use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde::Serialize;

use gproxy_server::AppState;

use crate::auth::authorize_admin;
use crate::error::HttpError;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub provider_count: usize,
    pub user_count: usize,
    pub timestamp_epoch: u64,
}

pub async fn health(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<HealthResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    let provider_count = state
        .engine()
        .store()
        .list_providers()
        .map(|v| v.len())
        .unwrap_or(0);

    let user_count = state.users_snapshot().len();

    let timestamp_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(Json(HealthResponse {
        status: "ok",
        provider_count,
        user_count,
        timestamp_epoch,
    }))
}
