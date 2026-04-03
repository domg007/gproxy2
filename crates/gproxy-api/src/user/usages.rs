use std::sync::Arc;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use gproxy_server::AppState;
use gproxy_storage::{UsageQuery, UsageQueryRow, UsageQueryCount, Scope};
use crate::auth::authenticate_user;
use crate::error::HttpError;

pub async fn query_usages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut query): Json<UsageQuery>,
) -> Result<Json<Vec<UsageQueryRow>>, HttpError> {
    let user_key = authenticate_user(&headers, &state)?;
    query.user_id = Scope::Eq(user_key.user_id);
    let rows = state.storage().query_usages(&query).await?;
    Ok(Json(rows))
}

pub async fn count_usages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut query): Json<UsageQuery>,
) -> Result<Json<UsageQueryCount>, HttpError> {
    let user_key = authenticate_user(&headers, &state)?;
    query.user_id = Scope::Eq(user_key.user_id);
    let count = state.storage().count_usages(&query).await?;
    Ok(Json(count))
}
