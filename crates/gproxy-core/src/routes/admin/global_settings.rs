use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};

use crate::{
    AppState, GlobalSettings, build_claudecode_spoof_client, build_http_client,
    normalize_spoof_emulation, normalize_update_source,
};

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
    Json(mut payload): Json<gproxy_storage::GlobalSettingsWrite>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    payload.spoof_emulation = normalize_spoof_emulation(Some(payload.spoof_emulation.as_str()));
    payload.update_source = normalize_update_source(Some(payload.update_source.as_str()));

    let global = GlobalSettings {
        host: payload.host.clone(),
        port: payload.port,
        proxy: payload.proxy.clone(),
        spoof_emulation: payload.spoof_emulation.clone(),
        update_source: payload.update_source.clone(),
        hf_token: payload.hf_token.clone(),
        hf_url: payload.hf_url.clone(),
        admin_key: payload.admin_key.clone(),
        mask_sensitive_info: payload.mask_sensitive_info,
        dsn: payload.dsn.clone(),
        data_dir: payload.data_dir.clone(),
    };

    let http = Arc::new(build_http_client(global.proxy.as_deref()).map_err(|err| {
        HttpError::new(
            StatusCode::BAD_REQUEST,
            format!("build standard upstream http client failed: {err}"),
        )
    })?);
    let spoof_http = Arc::new(
        build_claudecode_spoof_client(global.proxy.as_deref(), global.spoof_emulation.as_str())
            .map_err(|err| {
                HttpError::new(
                    StatusCode::BAD_REQUEST,
                    format!("build claudecode spoof http client failed: {err}"),
                )
            })?,
    );

    gproxy_admin::upsert_global_settings(&state.storage_writes, payload).await?;
    let mut snapshot = (*state.config.load_full()).clone();
    snapshot.global = global;
    state.replace_config(snapshot);
    state.replace_http_clients(http, spoof_http);
    Ok(Json(Ack { ok: true }))
}
