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
    pub admin_key: String,
    pub proxy: Option<String>,
    pub spoof_emulation: String,
    pub update_source: String,
    pub mask_sensitive_info: bool,
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
        admin_key: config.admin_key.clone(),
        proxy: config.proxy.clone(),
        spoof_emulation: config.spoof_emulation.clone(),
        update_source: config.update_source.clone(),
        mask_sensitive_info: config.mask_sensitive_info,
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

    // Check if proxy or spoof changed — need to rebuild engine clients
    let old_config = state.config();
    let proxy_changed = old_config.proxy != payload.proxy;
    let spoof_changed = old_config.spoof_emulation != payload.spoof_emulation;

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

    // Rebuild engine clients if proxy or spoof changed
    if proxy_changed || spoof_changed {
        let new_engine = state.engine().with_new_clients(
            payload.proxy.as_deref(),
            Some(payload.spoof_emulation.as_str()),
        );
        state.replace_engine(new_engine);
    }

    state
        .storage_writes()
        .enqueue(StorageWriteEvent::UpsertGlobalSettings(payload))
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;

    Ok(Json(AckResponse { ok: true, id: None }))
}
