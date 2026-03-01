use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;

use crate::AppState;

use super::{Ack, HttpError, authorize_admin};

pub(super) async fn get_global_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Option<gproxy_storage::GlobalSettingsRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    let row = gproxy_admin::get_global_settings(&storage).await?;
    Ok(Json(row))
}

pub(super) async fn upsert_global_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::GlobalSettingsWrite>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    gproxy_admin::upsert_global_settings(&state.storage_writes, payload).await?;
    Ok(Json(Ack { ok: true }))
}
