use std::sync::Arc;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use gproxy_server::AppState;
use gproxy_storage::{UsageQuery, UsageQueryRow, UsageQueryCount};
use crate::auth::authorize_admin;
use crate::error::HttpError;

pub async fn query_usages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<UsageQuery>,
) -> Result<Json<Vec<UsageQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let rows = state.storage().query_usages(&query).await?;
    Ok(Json(rows))
}

pub async fn count_usages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<UsageQuery>,
) -> Result<Json<UsageQueryCount>, HttpError> {
    authorize_admin(&headers, &state)?;
    let count = state.storage().count_usages(&query).await?;
    Ok(Json(count))
}
