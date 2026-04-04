use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde::Serialize;

use gproxy_server::AppState;
use gproxy_storage::{GlobalSettingsWrite, StorageWriteEvent};

use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};

#[derive(Serialize)]
pub struct GlobalSettingsResponse {
    pub host: String,
    pub port: u16,
    pub proxy: Option<String>,
    pub spoof_emulation: String,
    pub update_source: String,
    pub enable_usage: bool,
    pub enable_upstream_log: bool,
    pub enable_upstream_log_body: bool,
    pub enable_downstream_log: bool,
    pub enable_downstream_log_body: bool,
    pub dsn: String,
    pub data_dir: String,
}

pub async fn get_global_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<GlobalSettingsResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let config = state.config();
    Ok(Json(GlobalSettingsResponse {
        host: config.host.clone(),
        port: config.port,
        proxy: config.proxy.clone(),
        spoof_emulation: config.spoof_emulation.clone(),
        update_source: config.update_source.clone(),
        enable_usage: config.enable_usage,
        enable_upstream_log: config.enable_upstream_log,
        enable_upstream_log_body: config.enable_upstream_log_body,
        enable_downstream_log: config.enable_downstream_log,
        enable_downstream_log_body: config.enable_downstream_log_body,
        dsn: config.dsn.clone(),
        data_dir: config.data_dir.clone(),
    }))
}

pub async fn upsert_global_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<GlobalSettingsWrite>,
) -> Result<Json<AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    state.replace_config(gproxy_server::GlobalConfig {
        host: payload.host.clone(),
        port: payload.port,
        admin_key: payload.admin_key.clone(),
        proxy: payload.proxy.clone(),
        spoof_emulation: payload.spoof_emulation.clone(),
        update_source: payload.update_source.clone(),
        enable_usage: payload.enable_usage,
        enable_upstream_log: payload.enable_upstream_log,
        enable_upstream_log_body: payload.enable_upstream_log_body,
        enable_downstream_log: payload.enable_downstream_log,
        enable_downstream_log_body: payload.enable_downstream_log_body,
        dsn: payload.dsn.clone(),
        data_dir: payload.data_dir.clone(),
    });

    // Rebuild engine if any engine-relevant settings changed
    let new_engine = state.engine().with_settings(
        payload.proxy.as_deref(),
        Some(payload.spoof_emulation.as_str()),
        payload.enable_usage,
        payload.enable_upstream_log,
        payload.enable_upstream_log_body,
    );
    state.replace_engine(new_engine);

    state
        .storage_writes()
        .enqueue(StorageWriteEvent::UpsertGlobalSettings(payload))
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;

    Ok(Json(AckResponse { ok: true, id: None }))
}
