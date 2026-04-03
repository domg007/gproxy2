use std::sync::Arc;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use gproxy_server::AppState;
use gproxy_storage::{UserKeyQuery, UserKeyQueryRow};
use crate::auth::authenticate_user;
use crate::error::HttpError;

pub async fn query_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<UserKeyQueryRow>>, HttpError> {
    let user_key = authenticate_user(&headers, &state)?;
    let query = UserKeyQuery {
        user_id: gproxy_storage::Scope::Eq(user_key.user_id),
        ..Default::default()
    };
    let rows = state.storage().list_user_keys(&query).await?;
    Ok(Json(rows))
}
