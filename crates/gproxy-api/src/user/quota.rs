use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde::Serialize;

use gproxy_server::AppState;

use crate::auth::authenticate_user;
use crate::error::HttpError;

#[derive(Serialize)]
pub struct QuotaResponse {
    pub user_id: i64,
    /// Total allocated budget.
    pub quota: f64,
    /// Cumulative cost consumed.
    pub cost_used: f64,
    /// Remaining budget (quota - cost_used).
    pub remaining: f64,
}

/// Get the authenticated user's quota and cost.
pub async fn get_quota(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<QuotaResponse>, HttpError> {
    let user_key = authenticate_user(&headers, &state)?;
    let (quota, cost_used) = state.get_user_quota(user_key.user_id);
    Ok(Json(QuotaResponse {
        user_id: user_key.user_id,
        quota,
        cost_used,
        remaining: (quota - cost_used).max(0.0),
    }))
}
