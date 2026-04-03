use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;

use gproxy_server::AppState;
use gproxy_storage::{GlobalSettingsRow, GlobalSettingsWrite, StorageWriteEvent};

use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};

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
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<GlobalSettingsWrite>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    // Update in-memory config
    state.replace_config(gproxy_server::GlobalConfig {
        host: payload.host.clone(),
        port: payload.port,
        admin_key: payload.admin_key.clone(),
        proxy: payload.proxy.clone(),
        spoof_emulation: payload.spoof_emulation.clone(),
        update_source: payload.update_source.clone(),
        mask_sensitive_info: payload.mask_sensitive_info,
        dsn: payload.dsn.clone(),
        data_dir: payload.data_dir.clone(),
    });

    // Persist via write channel
    state
        .storage_writes()
        .enqueue(StorageWriteEvent::UpsertGlobalSettings(payload))
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;

    Ok(Json(AckResponse { ok: true, id: None }))
}
