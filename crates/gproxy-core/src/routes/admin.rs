use axum::Router;
use axum::routing::{get, post};
use gproxy_provider::{BuiltinChannelCredential, ModelCooldown, ProviderDispatchTable};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::HttpError;

mod auth;
use auth::*;
mod global_settings;
use global_settings::*;
mod system_update;
use system_update::*;
mod providers;
use providers::*;
mod credentials;
use credentials::*;
mod credential_statuses;
use credential_statuses::*;
mod users;
use users::*;
mod user_keys;
use user_keys::*;
mod requests;
use requests::*;
mod usages;
use usages::*;
mod config_toml;
use config_toml::*;

const X_API_KEY: &str = "x-api-key";
const ADMIN_USER_ID: i64 = 0;

#[derive(Debug, Serialize)]
struct Ack {
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct DeleteById {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct DeleteCredentialStatusPayload {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct UpsertUserPayload {
    #[serde(default)]
    id: Option<i64>,
    name: String,
    password: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct ImportTomlPayload {
    toml: String,
}

#[derive(Debug, Serialize)]
struct ExportBootstrapConfig {
    global: ExportGlobalConfig,
    runtime: ExportRuntimeConfig,
    channels: Vec<ExportChannelConfig>,
}

#[derive(Debug, Serialize)]
struct ExportGlobalConfig {
    host: String,
    port: u16,
    proxy: String,
    spoof_emulation: String,
    update_source: String,
    hf_token: String,
    hf_url: String,
    admin_key: String,
    mask_sensitive_info: bool,
    dsn: String,
    data_dir: String,
}

#[derive(Debug, Serialize)]
struct ExportRuntimeConfig {
    storage_write_queue_capacity: usize,
    storage_write_max_batch_size: usize,
    storage_write_aggregate_window_ms: u64,
}

#[derive(Debug, Serialize)]
struct ExportChannelConfig {
    id: String,
    enabled: bool,
    settings: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    dispatch: Option<ProviderDispatchTable>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    credentials: Vec<ExportCredentialConfig>,
}

#[derive(Debug, Serialize)]
struct ExportCredentialConfig {
    id: Option<String>,
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    builtin: Option<BuiltinChannelCredential>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<ExportCredentialState>,
}

#[derive(Debug, Serialize)]
struct ExportCredentialState {
    health: ExportCredentialHealth,
    #[serde(skip_serializing_if = "Option::is_none")]
    checked_at_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ExportCredentialHealth {
    Healthy,
    Partial { models: Vec<ModelCooldown> },
    Dead,
}

#[derive(Debug, Deserialize, Default)]
struct ImportBootstrapConfig {
    #[serde(default)]
    global: ImportGlobalConfig,
    #[serde(default)]
    channels: Vec<ImportChannelConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct ImportGlobalConfig {
    host: Option<String>,
    port: Option<u16>,
    proxy: Option<String>,
    spoof_emulation: Option<String>,
    update_source: Option<String>,
    hf_token: Option<String>,
    hf_url: Option<String>,
    admin_key: Option<String>,
    mask_sensitive_info: Option<bool>,
    dsn: Option<String>,
    data_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImportChannelConfig {
    id: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default = "default_settings_value")]
    settings: serde_json::Value,
    #[serde(default)]
    dispatch: Option<ProviderDispatchTable>,
    #[serde(default)]
    credentials: Vec<ImportCredentialConfig>,
}

#[derive(Debug, Deserialize)]
struct ImportCredentialConfig {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    builtin: Option<BuiltinChannelCredential>,
    #[serde(default)]
    state: Option<ImportCredentialState>,
}

#[derive(Debug, Deserialize)]
struct ImportCredentialState {
    #[serde(default)]
    health: ImportCredentialHealth,
    #[serde(default)]
    checked_at_unix_ms: Option<u64>,
    #[serde(default)]
    last_error: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ImportCredentialHealth {
    #[default]
    Healthy,
    Partial {
        #[serde(default)]
        models: Vec<ModelCooldown>,
    },
    Dead,
}

const fn default_true() -> bool {
    true
}

fn default_settings_value() -> serde_json::Value {
    serde_json::json!({})
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/global-settings", get(get_global_settings))
        .route("/global-settings/upsert", post(upsert_global_settings))
        .route("/system/self_update", post(system_self_update))
        .route("/system/latest_release", get(system_latest_release))
        .route("/config/export-toml", get(export_config_toml))
        .route("/config/import-toml", post(import_config_toml))
        .route("/providers/catalog", get(get_provider_channel_catalog))
        .route("/providers/query", post(query_providers))
        .route("/providers/upsert", post(upsert_provider))
        .route("/providers/delete", post(delete_provider))
        .route("/credentials/query", post(query_credentials))
        .route("/credentials/upsert", post(upsert_credential))
        .route("/credentials/delete", post(delete_credential))
        .route(
            "/credential-statuses/query",
            post(query_credential_statuses),
        )
        .route(
            "/credential-statuses/upsert",
            post(upsert_credential_status),
        )
        .route(
            "/credential-statuses/delete",
            post(delete_credential_status),
        )
        .route("/users/query", post(query_users))
        .route("/users/upsert", post(upsert_user))
        .route("/users/delete", post(delete_user))
        .route("/user-keys/query", post(query_user_keys))
        .route("/user-keys/generate", post(generate_user_key))
        .route("/user-keys/delete", post(delete_user_key))
        .route("/requests/upstream/query", post(query_upstream_requests))
        .route("/requests/upstream/count", post(count_upstream_requests))
        .route(
            "/requests/upstream/clear",
            post(clear_upstream_request_payloads),
        )
        .route(
            "/requests/downstream/query",
            post(query_downstream_requests),
        )
        .route(
            "/requests/downstream/count",
            post(count_downstream_requests),
        )
        .route(
            "/requests/downstream/clear",
            post(clear_downstream_request_payloads),
        )
        .route("/usages/query", post(query_usages))
        .route("/usages/count", post(count_usages))
        .route("/usages/summary", post(summarize_usages))
}
