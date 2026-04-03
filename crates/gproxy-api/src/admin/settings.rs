use std::sync::Arc;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use gproxy_server::AppState;
use gproxy_storage::GlobalSettingsRow;
use crate::auth::authorize_admin;
use crate::error::HttpError;

pub async fn get_global_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Option<GlobalSettingsRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.storage();
    let settings = storage.get_global_settings().await?;
    Ok(Json(settings))
}

pub async fn upsert_global_settings(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<crate::error::AckResponse>, HttpError> {
    // TODO: implement
    Err(HttpError::internal("not yet implemented"))
}
