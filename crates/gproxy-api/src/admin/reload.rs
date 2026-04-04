use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde::Serialize;

use gproxy_server::AppState;

use crate::auth::authorize_admin;
use crate::bootstrap::ReloadCounts;
use crate::error::HttpError;

#[derive(Serialize)]
pub struct ReloadResponse {
    pub ok: bool,
    #[serde(flatten)]
    pub counts: ReloadCounts,
}

/// Reload all in-memory caches from the database.
pub async fn reload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ReloadResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let counts = crate::bootstrap::reload_from_db(&state)
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    Ok(Json(ReloadResponse { ok: true, counts }))
}
