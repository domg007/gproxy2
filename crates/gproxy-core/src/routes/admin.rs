use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use gproxy_provider::{
    BuiltinChannelCredential, ChannelCredential, ChannelCredentialState, ChannelId,
    CredentialHealth, CredentialRef, ModelCooldown, ProviderDispatchTable,
};
use gproxy_storage::Scope;
use serde::Deserialize;
use serde::Serialize;

use crate::AppState;

use super::error::HttpError;

const X_API_KEY: &str = "x-api-key";
const ADMIN_USER_ID: i64 = 0;
const GPROXY_REPO_API_LATEST: &str = "https://api.github.com/repos/LeenHawk/gproxy/releases/latest";

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
        .route("/config/export-toml", get(export_config_toml))
        .route("/config/import-toml", post(import_config_toml))
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
        .route("/user-keys/upsert", post(upsert_user_key))
        .route("/user-keys/delete", post(delete_user_key))
        .route("/requests/upstream/query", post(query_upstream_requests))
        .route(
            "/requests/downstream/query",
            post(query_downstream_requests),
        )
        .route("/usages/query", post(query_usages))
        .route("/usages/summary", post(summarize_usages))
}

fn header_value<'a>(headers: &'a HeaderMap, name: &'static str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn authorize_admin(headers: &HeaderMap, state: &AppState) -> Result<(), HttpError> {
    let api_key =
        gproxy_admin::extract_api_key(header_value(headers, X_API_KEY)).map_err(HttpError::from)?;
    let Some(key) = state.authenticate_api_key_in_memory(api_key) else {
        return Err(HttpError::from(gproxy_admin::AdminApiError::Unauthorized));
    };
    if key.user_id != ADMIN_USER_ID {
        return Err(HttpError::from(gproxy_admin::AdminApiError::Forbidden));
    }
    Ok(())
}

async fn resolve_provider_channel_by_id(
    state: &AppState,
    id: i64,
) -> Result<Option<ChannelId>, HttpError> {
    let storage = state.load_storage();
    let rows = storage
        .list_providers(&gproxy_storage::ProviderQuery {
            channel: gproxy_storage::Scope::All,
            name: gproxy_storage::Scope::All,
            enabled: gproxy_storage::Scope::All,
            limit: None,
        })
        .await
        .map_err(|err| HttpError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    Ok(rows
        .into_iter()
        .find(|row| row.id == id)
        .map(|row| ChannelId::parse(row.channel.as_str())))
}

async fn get_global_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Option<gproxy_storage::GlobalSettingsRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    let row = gproxy_admin::get_global_settings(&storage).await?;
    Ok(Json(row))
}

async fn upsert_global_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::GlobalSettingsWrite>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    gproxy_admin::upsert_global_settings(&state.storage_writes, payload).await?;
    Ok(Json(Ack { ok: true }))
}

#[derive(Debug, Deserialize, Clone)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseInfo {
    tag_name: String,
    assets: Vec<GithubReleaseAsset>,
}

struct SelfUpdateResult {
    release_tag: String,
    asset_name: String,
    installed_to: String,
}

async fn system_self_update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, HttpError> {
    authorize_admin(&headers, &state)?;
    let proxy = state.config.load().global.proxy.clone();

    let result = self_update_to_latest_release(proxy).await.map_err(|err| {
        HttpError::new(
            StatusCode::BAD_GATEWAY,
            format!("self_update_failed: {err}"),
        )
    })?;

    schedule_self_restart().map_err(|err| {
        HttpError::new(
            StatusCode::BAD_GATEWAY,
            format!("self_restart_schedule_failed: {err}"),
        )
    })?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "from_version": env!("CARGO_PKG_VERSION"),
        "release_tag": result.release_tag,
        "asset": result.asset_name,
        "installed_to": result.installed_to,
        "restart_required": false,
        "restart_scheduled": true,
        "note": "Update prepared and process restart scheduled automatically."
    })))
}

async fn self_update_to_latest_release(proxy: Option<String>) -> Result<SelfUpdateResult, String> {
    #[cfg(windows)]
    {
        let _ = proxy;
        return Err("self_update_not_supported_on_windows_running_binary".to_string());
    }

    #[cfg(not(windows))]
    {
        let target_asset = target_release_asset_name()?;
        let client = build_self_update_client(proxy)?;
        let (release_tag, asset) = fetch_latest_release_asset(&client, &target_asset).await?;

        let zip_bytes =
            download_bytes_with_redirects(&client, &asset.browser_download_url, 8).await?;
        let binary_bytes = extract_binary_from_zip(&zip_bytes)?;
        install_binary_bytes(binary_bytes)?;

        let installed_to = env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "<unknown>".to_string());

        Ok(SelfUpdateResult {
            release_tag,
            asset_name: asset.name,
            installed_to,
        })
    }
}

async fn fetch_latest_release_asset(
    client: &wreq::Client,
    target_asset: &str,
) -> Result<(String, GithubReleaseAsset), String> {
    let release_resp = client
        .get(GPROXY_REPO_API_LATEST)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", concat!("gproxy/", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .map_err(|err| format!("fetch_latest_release: {err}"))?;

    if !release_resp.status().is_success() {
        let status = release_resp.status();
        let body = release_resp
            .bytes()
            .await
            .map(|body| String::from_utf8_lossy(&body).to_string())
            .unwrap_or_else(|_| String::new());
        return Err(format!("fetch_latest_release_status_{status}: {body}"));
    }

    let release_body = release_resp
        .bytes()
        .await
        .map_err(|err| format!("read_latest_release_body: {err}"))?;
    let release: GithubReleaseInfo = serde_json::from_slice(&release_body)
        .map_err(|err| format!("parse_latest_release_json: {err}"))?;
    let asset = release
        .assets
        .iter()
        .find(|item| item.name == target_asset)
        .cloned()
        .ok_or_else(|| {
            let names = release
                .assets
                .iter()
                .map(|item| item.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            format!("asset_not_found_for_target:{target_asset}; available=[{names}]")
        })?;

    Ok((release.tag_name, asset))
}

fn build_self_update_client(proxy: Option<String>) -> Result<wreq::Client, String> {
    let proxy = proxy
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let mut builder = wreq::Client::builder();
    if let Some(proxy) = proxy.as_deref() {
        let parsed = wreq::Proxy::all(proxy).map_err(|err| format!("invalid_proxy:{err}"))?;
        builder = builder.proxy(parsed);
    }
    builder
        .build()
        .map_err(|err| format!("build_http_client: {err}"))
}

async fn download_bytes_with_redirects(
    client: &wreq::Client,
    url: &str,
    max_redirects: usize,
) -> Result<Vec<u8>, String> {
    let mut current = url.to_string();

    for _ in 0..=max_redirects {
        let response = client
            .get(&current)
            .header("accept", "application/octet-stream")
            .header("user-agent", concat!("gproxy/", env!("CARGO_PKG_VERSION")))
            .send()
            .await
            .map_err(|err| format!("download_asset:{current}:{err}"))?;
        let status = response.status();

        if status.is_success() {
            return response
                .bytes()
                .await
                .map(|body| body.to_vec())
                .map_err(|err| format!("read_asset_body:{current}:{err}"));
        }

        if status.is_redirection() {
            let Some(location) = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                return Err(format!("redirect_without_location:{status}:{current}"));
            };

            if !location.starts_with("http://") && !location.starts_with("https://") {
                return Err(format!(
                    "relative_redirect_unsupported:{status}:{current}:{location}"
                ));
            }

            current = location.to_string();
            continue;
        }

        let body = response
            .bytes()
            .await
            .map(|body| String::from_utf8_lossy(&body).to_string())
            .unwrap_or_else(|_| String::new());
        return Err(format!("download_asset_status_{status}:{current}: {body}"));
    }

    Err(format!(
        "download_asset_too_many_redirects:start_url={url}:max={max_redirects}"
    ))
}

fn target_release_asset_name() -> Result<String, String> {
    let arch = match env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => return Err(format!("unsupported_arch:{other}")),
    };

    let os = env::consts::OS;
    let name = match os {
        "linux" => {
            #[cfg(target_env = "musl")]
            let libc_suffix = "-musl";
            #[cfg(not(target_env = "musl"))]
            let libc_suffix = "";
            format!("gproxy-linux-{arch}{libc_suffix}.zip")
        }
        "macos" => format!("gproxy-macos-{arch}.zip"),
        "windows" => format!("gproxy-windows-{arch}.zip"),
        other => return Err(format!("unsupported_os:{other}")),
    };
    Ok(name)
}

fn extract_binary_from_zip(zip_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let cursor = std::io::Cursor::new(zip_bytes.to_vec());
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|err| format!("open_zip_archive:{err}"))?;

    let exe_name = if cfg!(windows) {
        "gproxy.exe"
    } else {
        "gproxy"
    };
    let mut file = archive
        .by_name(exe_name)
        .map_err(|err| format!("zip_entry_not_found:{exe_name}:{err}"))?;

    let mut out = Vec::new();
    file.read_to_end(&mut out)
        .map_err(|err| format!("read_zip_entry:{err}"))?;
    if out.is_empty() {
        return Err("zip_entry_empty".to_string());
    }
    Ok(out)
}

fn install_binary_bytes(binary: Vec<u8>) -> Result<(), String> {
    let current = env::current_exe().map_err(|err| format!("current_exe:{err}"))?;
    let parent = current
        .parent()
        .ok_or_else(|| "current_exe_parent_missing".to_string())?;
    let temp = temp_update_path(parent);

    fs::write(&temp, &binary)
        .map_err(|err| format!("write_temp_binary:{}:{err}", temp.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&current)
            .map(|meta| meta.permissions().mode())
            .unwrap_or(0o755);
        fs::set_permissions(&temp, fs::Permissions::from_mode(mode))
            .map_err(|err| format!("set_temp_permissions:{}:{err}", temp.display()))?;
    }

    fs::rename(&temp, &current).map_err(|err| {
        let _ = fs::remove_file(&temp);
        format!(
            "replace_binary_failed:{}->{}:{err}",
            temp.display(),
            current.display()
        )
    })?;

    Ok(())
}

fn temp_update_path(parent: &std::path::Path) -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    if cfg!(windows) {
        parent.join(format!("gproxy-update-{pid}-{nanos}.exe.new"))
    } else {
        parent.join(format!(".gproxy-update-{pid}-{nanos}.new"))
    }
}

fn schedule_self_restart() -> Result<(), String> {
    let exe = env::current_exe().map_err(|err| format!("current_exe_for_restart:{err}"))?;
    let args: Vec<std::ffi::OsString> = env::args_os().skip(1).collect();

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        restart_current_process(exe, args);
    });

    Ok(())
}

#[cfg(unix)]
fn restart_current_process(exe: PathBuf, args: Vec<std::ffi::OsString>) {
    use std::os::unix::process::CommandExt;

    let mut cmd = std::process::Command::new(&exe);
    cmd.args(&args);
    let err = cmd.exec();
    eprintln!("self_update exec failed for {}: {err}", exe.display());
    std::process::exit(1);
}

#[cfg(not(unix))]
fn restart_current_process(exe: PathBuf, args: Vec<std::ffi::OsString>) {
    match std::process::Command::new(&exe).args(&args).spawn() {
        Ok(_) => std::process::exit(0),
        Err(err) => {
            eprintln!(
                "self_update spawn failed for {} with args {:?}: {err}",
                exe.display(),
                args
            );
            std::process::exit(1);
        }
    }
}

async fn export_config_toml(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    authorize_admin(&headers, &state)?;

    let snapshot = state.config.load();
    let storage = state.load_storage();
    let mut providers = gproxy_admin::query_providers(
        &storage,
        gproxy_storage::ProviderQuery {
            channel: Scope::All,
            name: Scope::All,
            enabled: Scope::All,
            limit: None,
        },
    )
    .await?;
    providers.sort_by_key(|row| row.id);

    let mut credentials = gproxy_admin::query_credentials(
        &storage,
        gproxy_storage::CredentialQuery {
            provider_id: Scope::All,
            kind: Scope::All,
            enabled: Scope::All,
            limit: None,
        },
    )
    .await?;
    credentials.sort_by_key(|row| row.id);

    let statuses = gproxy_admin::query_credential_statuses(
        &storage,
        gproxy_storage::CredentialStatusQuery {
            id: Scope::All,
            credential_id: Scope::All,
            channel: Scope::All,
            health_kind: Scope::All,
            limit: None,
        },
    )
    .await?;
    let status_by_credential_channel = statuses
        .into_iter()
        .map(|row| ((row.credential_id, row.channel.clone()), row))
        .collect::<std::collections::HashMap<_, _>>();

    let channels = providers
        .into_iter()
        .map(|provider| {
            let channel_id = ChannelId::parse(provider.channel.as_str());
            let dispatch = serde_json::from_value::<ProviderDispatchTable>(provider.dispatch_json)
                .map_err(|err| {
                    HttpError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!(
                            "invalid dispatch_json for provider channel={} id={}: {err}",
                            provider.channel, provider.id
                        ),
                    )
                })?;
            let default_dispatch = match channel_id {
                ChannelId::Builtin(builtin) => ProviderDispatchTable::default_for_builtin(builtin),
                ChannelId::Custom(_) => ProviderDispatchTable::default_for_custom(),
            };
            let dispatch = (dispatch != default_dispatch).then_some(dispatch);

            let provider_credentials = credentials
                .iter()
                .filter(|item| item.provider_id == provider.id)
                .map(|row| {
                    let credential = serde_json::from_value::<ChannelCredential>(
                        row.secret_json.clone(),
                    )
                    .map_err(|err| {
                        HttpError::new(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!(
                                "invalid credential secret_json for credential_id={}: {err}",
                                row.id
                            ),
                        )
                    })?;

                    let (secret, builtin) = split_export_credential(credential);
                    let state = status_by_credential_channel
                        .get(&(row.id, provider.channel.clone()))
                        .map(export_credential_state);

                    Ok::<ExportCredentialConfig, HttpError>(ExportCredentialConfig {
                        id: Some(row.id.to_string()),
                        label: row.name.clone(),
                        secret,
                        builtin,
                        state,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;

            Ok::<ExportChannelConfig, HttpError>(ExportChannelConfig {
                id: provider.channel,
                enabled: provider.enabled,
                settings: provider.settings_json,
                dispatch,
                credentials: provider_credentials,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    const DEFAULT_STORAGE_WRITE_QUEUE_CAPACITY: usize = 4096;
    const DEFAULT_STORAGE_WRITE_MAX_BATCH_SIZE: usize = 1024;
    const DEFAULT_STORAGE_WRITE_AGGREGATE_WINDOW_MS: u64 = 25;

    let config = ExportBootstrapConfig {
        global: ExportGlobalConfig {
            host: snapshot.global.host.clone(),
            port: snapshot.global.port,
            proxy: snapshot.global.proxy.clone().unwrap_or_default(),
            hf_token: snapshot.global.hf_token.clone().unwrap_or_default(),
            hf_url: snapshot.global.hf_url.clone().unwrap_or_default(),
            admin_key: snapshot.global.admin_key.clone(),
            mask_sensitive_info: snapshot.global.mask_sensitive_info,
            dsn: snapshot.global.dsn.clone(),
            data_dir: snapshot.global.data_dir.clone(),
        },
        runtime: ExportRuntimeConfig {
            storage_write_queue_capacity: DEFAULT_STORAGE_WRITE_QUEUE_CAPACITY,
            storage_write_max_batch_size: DEFAULT_STORAGE_WRITE_MAX_BATCH_SIZE,
            storage_write_aggregate_window_ms: DEFAULT_STORAGE_WRITE_AGGREGATE_WINDOW_MS,
        },
        channels,
    };

    let text = toml::to_string_pretty(&config).map_err(|err| {
        HttpError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize toml failed: {err}"),
        )
    })?;

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; charset=utf-8")
        .header(
            "content-disposition",
            "attachment; filename=\"gproxy.toml\"",
        )
        .body(Body::from(text))
        .map_err(|err| {
            HttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("build response failed: {err}"),
            )
        })
}

async fn import_config_toml(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ImportTomlPayload>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;

    let parsed: ImportBootstrapConfig = toml::from_str(payload.toml.as_str()).map_err(|err| {
        HttpError::new(
            StatusCode::BAD_REQUEST,
            format!("invalid toml payload: {err}"),
        )
    })?;

    apply_imported_global(&state, &parsed.global).await?;
    apply_imported_channels(&state, &parsed.channels).await?;
    Ok(Json(Ack { ok: true }))
}

async fn query_providers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::ProviderQuery>,
) -> Result<Json<Vec<gproxy_storage::ProviderQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(gproxy_admin::query_providers(&storage, query).await?))
}

async fn upsert_provider(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::ProviderWrite>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    let channel = ChannelId::parse(payload.channel.as_str());
    let settings = gproxy_provider::parse_provider_settings_json_for_channel(
        &channel,
        payload.settings_json.as_str(),
    )
    .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?;
    let dispatch = serde_json::from_str::<gproxy_provider::ProviderDispatchTable>(
        payload.dispatch_json.as_str(),
    )
    .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?;
    state.upsert_provider_in_memory(channel, settings, dispatch, payload.enabled);
    gproxy_admin::upsert_provider(&state.storage_writes, payload).await?;
    Ok(Json(Ack { ok: true }))
}

async fn delete_provider(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteById>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    if let Some(channel) = resolve_provider_channel_by_id(&state, payload.id).await? {
        state.delete_provider_in_memory(&channel);
    }
    gproxy_admin::delete_provider(&state.storage_writes, payload.id).await?;
    Ok(Json(Ack { ok: true }))
}

async fn query_credentials(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::CredentialQuery>,
) -> Result<Json<Vec<gproxy_storage::CredentialQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::query_credentials(&storage, query).await?,
    ))
}

async fn upsert_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut payload): Json<gproxy_storage::CredentialWrite>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    if let Some(channel) = resolve_provider_channel_by_id(&state, payload.provider_id).await? {
        let mut credential = serde_json::from_str::<gproxy_provider::ChannelCredential>(
            payload.secret_json.as_str(),
        )
        .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?;

        maybe_detect_and_fill_project_id(&state, &channel, &mut credential).await?;
        payload.secret_json = serde_json::to_string(&credential)
            .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?;

        state.upsert_provider_credential_in_memory(
            &channel,
            gproxy_provider::CredentialRef {
                id: payload.id,
                label: payload.name.clone(),
                credential,
            },
        );
    }
    gproxy_admin::upsert_credential(&state.storage_writes, payload).await?;
    Ok(Json(Ack { ok: true }))
}

async fn maybe_detect_and_fill_project_id(
    state: &AppState,
    channel: &ChannelId,
    credential: &mut gproxy_provider::ChannelCredential,
) -> Result<(), HttpError> {
    let settings = if let Some(provider) = state.config.load().providers.get(channel) {
        provider.settings.clone()
    } else {
        gproxy_provider::parse_provider_settings_json_for_channel(channel, "{}")
            .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?
    };

    match (channel, credential) {
        (
            ChannelId::Builtin(gproxy_provider::BuiltinChannel::GeminiCli),
            gproxy_provider::ChannelCredential::Builtin(
                gproxy_provider::BuiltinChannelCredential::GeminiCli(value),
            ),
        ) if value.project_id.trim().is_empty() => {
            let http = state.load_http();
            gproxy_provider::channels::geminicli::ensure_geminicli_project_id(
                http.as_ref(),
                &settings,
                value,
            )
            .await
            .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        (
            ChannelId::Builtin(gproxy_provider::BuiltinChannel::Antigravity),
            gproxy_provider::ChannelCredential::Builtin(
                gproxy_provider::BuiltinChannelCredential::Antigravity(value),
            ),
        ) if value.project_id.trim().is_empty() => {
            let http = state.load_http();
            gproxy_provider::channels::antigravity::ensure_antigravity_project_id(
                http.as_ref(),
                &settings,
                value,
            )
            .await
            .map_err(|err| HttpError::new(StatusCode::BAD_REQUEST, err.to_string()))?;
        }
        _ => {}
    }

    Ok(())
}

async fn delete_credential(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteById>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    let rows = storage
        .list_credentials(&gproxy_storage::CredentialQuery {
            provider_id: gproxy_storage::Scope::All,
            kind: gproxy_storage::Scope::All,
            enabled: gproxy_storage::Scope::All,
            limit: None,
        })
        .await
        .map_err(|err| HttpError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    if let Some(credential_row) = rows.into_iter().find(|row| row.id == payload.id)
        && let Some(channel) =
            resolve_provider_channel_by_id(&state, credential_row.provider_id).await?
    {
        let _ = state.delete_provider_credential_in_memory(&channel, payload.id);
    }
    gproxy_admin::delete_credential(&state.storage_writes, payload.id).await?;
    Ok(Json(Ack { ok: true }))
}

async fn query_credential_statuses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::CredentialStatusQuery>,
) -> Result<Json<Vec<gproxy_storage::CredentialStatusQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::query_credential_statuses(&storage, query).await?,
    ))
}

async fn upsert_credential_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::CredentialStatusWrite>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    let health =
        parse_credential_health(payload.health_kind.as_str(), payload.health_json.as_deref())?;
    let checked_at_unix_ms = payload
        .checked_at_unix_ms
        .and_then(|value| (value >= 0).then_some(value as u64));
    state.upsert_credential_state(ChannelCredentialState {
        channel: ChannelId::parse(payload.channel.as_str()),
        credential_id: payload.credential_id,
        health,
        checked_at_unix_ms,
        last_error: payload.last_error.clone(),
    });
    gproxy_admin::upsert_credential_status(&state.storage_writes, payload).await?;
    Ok(Json(Ack { ok: true }))
}

async fn delete_credential_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteCredentialStatusPayload>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    if let Some(row) = gproxy_admin::query_credential_statuses(
        &storage,
        gproxy_storage::CredentialStatusQuery {
            id: gproxy_storage::Scope::Eq(payload.id),
            credential_id: gproxy_storage::Scope::All,
            channel: gproxy_storage::Scope::All,
            health_kind: gproxy_storage::Scope::All,
            limit: Some(1),
        },
    )
    .await?
    .into_iter()
    .next()
    {
        state
            .credential_states
            .remove(&ChannelId::parse(row.channel.as_str()), row.credential_id);
    }
    gproxy_admin::delete_credential_status(&state.storage_writes, payload.id).await?;
    Ok(Json(Ack { ok: true }))
}

fn parse_credential_health(
    kind: &str,
    health_json: Option<&str>,
) -> Result<CredentialHealth, HttpError> {
    let normalized = kind.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "healthy" => Ok(CredentialHealth::Healthy),
        "dead" => Ok(CredentialHealth::Dead),
        "partial" => {
            let raw = health_json.unwrap_or("[]").trim();
            if raw.is_empty() {
                return Ok(CredentialHealth::Partial { models: Vec::new() });
            }
            if let Ok(models) = serde_json::from_str::<Vec<ModelCooldown>>(raw) {
                return Ok(CredentialHealth::Partial { models });
            }
            let value = serde_json::from_str::<serde_json::Value>(raw).map_err(|err| {
                HttpError::new(
                    StatusCode::BAD_REQUEST,
                    format!("invalid health_json: {err}"),
                )
            })?;
            let models = value.get("models").cloned().unwrap_or(value);
            let parsed = serde_json::from_value::<Vec<ModelCooldown>>(models).map_err(|err| {
                HttpError::new(
                    StatusCode::BAD_REQUEST,
                    format!("invalid partial models: {err}"),
                )
            })?;
            Ok(CredentialHealth::Partial { models: parsed })
        }
        _ => Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            format!("invalid health_kind: {kind}"),
        )),
    }
}

async fn query_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UserQuery>,
) -> Result<Json<Vec<gproxy_storage::UserQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let users = state.load_users();
    Ok(Json(gproxy_admin::query_users(&users, query).await?))
}

async fn upsert_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UpsertUserPayload>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(HttpError::from(gproxy_admin::AdminApiError::InvalidInput(
            "user name cannot be empty".to_string(),
        )));
    }
    let id = payload.id.unwrap_or_else(|| {
        state
            .load_users()
            .iter()
            .map(|row| row.id)
            .max()
            .unwrap_or(-1)
            + 1
    });
    let write = gproxy_storage::UserWrite {
        id,
        name: name.to_string(),
        enabled: payload.enabled,
    };
    state.upsert_user_in_memory(write.clone());
    gproxy_admin::upsert_user(&state.storage_writes, write).await?;
    Ok(Json(Ack { ok: true }))
}

async fn delete_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteById>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    state.delete_user_in_memory(payload.id);
    gproxy_admin::delete_user(&state.storage_writes, payload.id).await?;
    Ok(Json(Ack { ok: true }))
}

async fn query_user_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UserKeyQuery>,
) -> Result<Json<Vec<gproxy_storage::UserKeyQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let keys = state.load_keys();
    Ok(Json(gproxy_admin::query_user_keys(&keys, query).await?))
}

async fn upsert_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gproxy_storage::UserKeyWrite>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    let keys = state.load_keys();
    let row = gproxy_admin::upsert_user_key(&keys, &state.storage_writes, payload).await?;
    state.upsert_user_key_in_memory(row);
    Ok(Json(Ack { ok: true }))
}

async fn delete_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteById>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    state.delete_user_key_in_memory(payload.id);
    gproxy_admin::delete_user_key(&state.storage_writes, payload.id).await?;
    Ok(Json(Ack { ok: true }))
}

async fn query_upstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UpstreamRequestQuery>,
) -> Result<Json<Vec<gproxy_storage::UpstreamRequestQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::query_upstream_requests(&storage, query).await?,
    ))
}

async fn query_downstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::DownstreamRequestQuery>,
) -> Result<Json<Vec<gproxy_storage::DownstreamRequestQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(
        gproxy_admin::query_downstream_requests(&storage, query).await?,
    ))
}

async fn query_usages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UsageQuery>,
) -> Result<Json<Vec<gproxy_storage::UsageQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(gproxy_admin::query_usages(&storage, query).await?))
}

async fn summarize_usages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UsageQuery>,
) -> Result<Json<gproxy_storage::UsageSummary>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.load_storage();
    Ok(Json(gproxy_admin::summarize_usages(&storage, query).await?))
}

async fn apply_imported_global(
    state: &Arc<AppState>,
    imported: &ImportGlobalConfig,
) -> Result<(), HttpError> {
    let mut global = state.config.load().global.clone();

    if let Some(host) = imported.host.as_ref() {
        global.host = host.clone();
    }
    if let Some(port) = imported.port {
        global.port = port;
    }
    if let Some(proxy) = imported.proxy.as_ref() {
        global.proxy = if proxy.trim().is_empty() {
            None
        } else {
            Some(proxy.clone())
        };
    }
    if let Some(hf_token) = imported.hf_token.as_ref() {
        global.hf_token = if hf_token.trim().is_empty() {
            None
        } else {
            Some(hf_token.clone())
        };
    }
    if let Some(hf_url) = imported.hf_url.as_ref() {
        global.hf_url = if hf_url.trim().is_empty() {
            None
        } else {
            Some(hf_url.clone())
        };
    }
    if let Some(admin_key) = imported.admin_key.as_ref() {
        global.admin_key = admin_key.clone();
    }
    if let Some(mask_sensitive_info) = imported.mask_sensitive_info {
        global.mask_sensitive_info = mask_sensitive_info;
    }
    if let Some(dsn) = imported.dsn.as_ref() {
        global.dsn = dsn.clone();
    }
    if let Some(data_dir) = imported.data_dir.as_ref() {
        global.data_dir = data_dir.clone();
    }

    let mut snapshot = (*state.config.load_full()).clone();
    snapshot.global = global.clone();
    state.replace_config(snapshot);

    gproxy_admin::upsert_global_settings(
        &state.storage_writes,
        gproxy_storage::GlobalSettingsWrite {
            host: global.host,
            port: global.port,
            proxy: global.proxy,
            hf_token: global.hf_token,
            hf_url: global.hf_url,
            admin_key: global.admin_key,
            mask_sensitive_info: global.mask_sensitive_info,
            dsn: global.dsn,
            data_dir: global.data_dir,
        },
    )
    .await?;

    Ok(())
}

async fn apply_imported_channels(
    state: &Arc<AppState>,
    channels: &[ImportChannelConfig],
) -> Result<(), HttpError> {
    if channels.is_empty() {
        return Ok(());
    }

    let storage = state.load_storage();
    let existing_providers = gproxy_admin::query_providers(
        &storage,
        gproxy_storage::ProviderQuery {
            channel: Scope::All,
            name: Scope::All,
            enabled: Scope::All,
            limit: None,
        },
    )
    .await?;
    let mut provider_id_by_channel = existing_providers
        .iter()
        .map(|row| (row.channel.clone(), row.id))
        .collect::<HashMap<_, _>>();
    let mut next_provider_id = existing_providers
        .iter()
        .map(|row| row.id)
        .max()
        .unwrap_or(-1)
        + 1;

    let existing_credentials = gproxy_admin::query_credentials(
        &storage,
        gproxy_storage::CredentialQuery {
            provider_id: Scope::All,
            kind: Scope::All,
            enabled: Scope::All,
            limit: None,
        },
    )
    .await?;
    let mut credential_id_by_provider_label = existing_credentials
        .iter()
        .filter_map(|row| {
            row.name
                .as_ref()
                .map(|label| ((row.provider_id, label.clone()), row.id))
        })
        .collect::<HashMap<_, _>>();
    let mut used_credential_ids = existing_credentials
        .iter()
        .map(|row| row.id)
        .collect::<HashSet<_>>();
    let mut next_credential_id = existing_credentials
        .iter()
        .map(|row| row.id)
        .max()
        .unwrap_or(-1)
        + 1;

    let existing_statuses = gproxy_admin::query_credential_statuses(
        &storage,
        gproxy_storage::CredentialStatusQuery {
            id: Scope::All,
            credential_id: Scope::All,
            channel: Scope::All,
            health_kind: Scope::All,
            limit: None,
        },
    )
    .await?;
    let status_id_by_credential_channel = existing_statuses
        .into_iter()
        .map(|row| ((row.credential_id, row.channel.clone()), row.id))
        .collect::<HashMap<_, _>>();

    for item in channels {
        let channel_name = item.id.trim();
        if channel_name.is_empty() {
            return Err(HttpError::new(
                StatusCode::BAD_REQUEST,
                "channel id cannot be empty".to_string(),
            ));
        }
        let channel = ChannelId::parse(channel_name);
        let provider_id = if let Some(existing) = provider_id_by_channel.get(channel_name).copied()
        {
            existing
        } else {
            let id = next_provider_id;
            next_provider_id += 1;
            provider_id_by_channel.insert(channel_name.to_string(), id);
            id
        };

        let settings =
            gproxy_provider::parse_provider_settings_value_for_channel(&channel, &item.settings)
                .map_err(|err| {
                    HttpError::new(
                        StatusCode::BAD_REQUEST,
                        format!("invalid channel settings for {channel_name}: {err}"),
                    )
                })?;
        let dispatch = item.dispatch.clone().unwrap_or_else(|| match channel {
            ChannelId::Builtin(builtin) => ProviderDispatchTable::default_for_builtin(builtin),
            ChannelId::Custom(_) => ProviderDispatchTable::default_for_custom(),
        });

        state.upsert_provider_in_memory(
            channel.clone(),
            settings.clone(),
            dispatch.clone(),
            item.enabled,
        );
        let settings_json =
            gproxy_provider::provider_settings_to_json_string(&settings).map_err(|err| {
                HttpError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("serialize provider settings failed for {channel_name}: {err}"),
                )
            })?;
        let dispatch_json = serde_json::to_string(&dispatch).map_err(|err| {
            HttpError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("serialize dispatch failed for {channel_name}: {err}"),
            )
        })?;
        gproxy_admin::upsert_provider(
            &state.storage_writes,
            gproxy_storage::ProviderWrite {
                id: provider_id,
                name: channel_name.to_string(),
                channel: channel_name.to_string(),
                settings_json,
                dispatch_json,
                enabled: item.enabled,
            },
        )
        .await?;

        for credential_item in &item.credentials {
            let credential = build_import_channel_credential(&channel, credential_item)?;
            let credential_id = resolve_import_credential_id(
                provider_id,
                credential_item,
                &credential_id_by_provider_label,
                &mut used_credential_ids,
                &mut next_credential_id,
            );

            if let Some(label) = credential_item.label.clone() {
                credential_id_by_provider_label.insert((provider_id, label), credential_id);
            }

            state.upsert_provider_credential_in_memory(
                &channel,
                CredentialRef {
                    id: credential_id,
                    label: credential_item.label.clone(),
                    credential: credential.clone(),
                },
            );
            gproxy_admin::upsert_credential(
                &state.storage_writes,
                gproxy_storage::CredentialWrite {
                    id: credential_id,
                    provider_id,
                    name: credential_item.label.clone(),
                    kind: credential_kind_for_storage(&credential),
                    settings_json: None,
                    secret_json: serde_json::to_string(&credential).map_err(|err| {
                        HttpError::new(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("serialize credential failed: {err}"),
                        )
                    })?,
                    enabled: true,
                },
            )
            .await?;

            if let Some(status) = credential_item.state.as_ref() {
                let (runtime_health, health_kind, health_json) =
                    import_health_to_storage(&status.health);
                state.upsert_credential_state(ChannelCredentialState {
                    channel: channel.clone(),
                    credential_id,
                    health: runtime_health,
                    checked_at_unix_ms: status.checked_at_unix_ms,
                    last_error: status.last_error.clone(),
                });
                gproxy_admin::upsert_credential_status(
                    &state.storage_writes,
                    gproxy_storage::CredentialStatusWrite {
                        id: status_id_by_credential_channel
                            .get(&(credential_id, channel_name.to_string()))
                            .copied(),
                        credential_id,
                        channel: channel_name.to_string(),
                        health_kind,
                        health_json,
                        checked_at_unix_ms: status
                            .checked_at_unix_ms
                            .map(|value| value.min(i64::MAX as u64) as i64),
                        last_error: status.last_error.clone(),
                    },
                )
                .await?;
            }
        }
    }

    Ok(())
}

fn resolve_import_credential_id(
    provider_id: i64,
    credential: &ImportCredentialConfig,
    credential_id_by_provider_label: &HashMap<(i64, String), i64>,
    used_credential_ids: &mut HashSet<i64>,
    next_credential_id: &mut i64,
) -> i64 {
    if let Some(id) = credential
        .id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<i64>().ok())
    {
        used_credential_ids.insert(id);
        if id >= *next_credential_id {
            *next_credential_id = id + 1;
        }
        return id;
    }

    if let Some(label) = credential
        .label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        && let Some(id) = credential_id_by_provider_label
            .get(&(provider_id, label))
            .copied()
    {
        used_credential_ids.insert(id);
        return id;
    }

    while used_credential_ids.contains(next_credential_id) {
        *next_credential_id += 1;
    }
    let id = *next_credential_id;
    used_credential_ids.insert(id);
    *next_credential_id += 1;
    id
}

fn build_import_channel_credential(
    channel: &ChannelId,
    credential: &ImportCredentialConfig,
) -> Result<ChannelCredential, HttpError> {
    if let Some(builtin) = credential.builtin.clone() {
        return match channel {
            ChannelId::Builtin(_) => Ok(ChannelCredential::Builtin(builtin)),
            ChannelId::Custom(_) => Err(HttpError::new(
                StatusCode::BAD_REQUEST,
                "custom channel does not support builtin credential payload".to_string(),
            )),
        };
    }

    let Some(secret) = credential
        .secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(HttpError::new(
            StatusCode::BAD_REQUEST,
            "credential requires either builtin or secret".to_string(),
        ));
    };

    match channel {
        ChannelId::Custom(_) => Ok(ChannelCredential::Custom(
            gproxy_provider::CustomChannelCredential {
                api_key: secret.to_string(),
            },
        )),
        ChannelId::Builtin(builtin) => match builtin {
            gproxy_provider::BuiltinChannel::OpenAi => Ok(ChannelCredential::Builtin(
                BuiltinChannelCredential::OpenAi(
                    gproxy_provider::channels::openai::OpenAiCredential {
                        api_key: secret.to_string(),
                    },
                ),
            )),
            gproxy_provider::BuiltinChannel::Claude => Ok(ChannelCredential::Builtin(
                BuiltinChannelCredential::Claude(
                    gproxy_provider::channels::claude::ClaudeCredential {
                        api_key: secret.to_string(),
                    },
                ),
            )),
            gproxy_provider::BuiltinChannel::AiStudio => Ok(ChannelCredential::Builtin(
                BuiltinChannelCredential::AiStudio(
                    gproxy_provider::channels::aistudio::AiStudioCredential {
                        api_key: secret.to_string(),
                    },
                ),
            )),
            gproxy_provider::BuiltinChannel::VertexExpress => Ok(ChannelCredential::Builtin(
                BuiltinChannelCredential::VertexExpress(
                    gproxy_provider::channels::vertexexpress::VertexExpressCredential {
                        api_key: secret.to_string(),
                    },
                ),
            )),
            gproxy_provider::BuiltinChannel::Nvidia => Ok(ChannelCredential::Builtin(
                BuiltinChannelCredential::Nvidia(
                    gproxy_provider::channels::nvidia::NvidiaCredential {
                        api_key: secret.to_string(),
                    },
                ),
            )),
            gproxy_provider::BuiltinChannel::Deepseek => Ok(ChannelCredential::Builtin(
                BuiltinChannelCredential::Deepseek(
                    gproxy_provider::channels::deepseek::DeepseekCredential {
                        api_key: secret.to_string(),
                    },
                ),
            )),
            gproxy_provider::BuiltinChannel::Vertex
            | gproxy_provider::BuiltinChannel::GeminiCli
            | gproxy_provider::BuiltinChannel::ClaudeCode
            | gproxy_provider::BuiltinChannel::Codex
            | gproxy_provider::BuiltinChannel::Antigravity => Err(HttpError::new(
                StatusCode::BAD_REQUEST,
                format!(
                    "channel {} requires builtin credential object",
                    channel.as_str()
                ),
            )),
        },
    }
}

fn import_health_to_storage(
    health: &ImportCredentialHealth,
) -> (CredentialHealth, String, Option<String>) {
    match health {
        ImportCredentialHealth::Healthy => (CredentialHealth::Healthy, "healthy".to_string(), None),
        ImportCredentialHealth::Dead => (CredentialHealth::Dead, "dead".to_string(), None),
        ImportCredentialHealth::Partial { models } => (
            CredentialHealth::Partial {
                models: models.clone(),
            },
            "partial".to_string(),
            serde_json::to_string(models).ok(),
        ),
    }
}

fn credential_kind_for_storage(credential: &ChannelCredential) -> String {
    match credential {
        ChannelCredential::Builtin(BuiltinChannelCredential::OpenAi(_)) => "builtin/openai",
        ChannelCredential::Builtin(BuiltinChannelCredential::Claude(_)) => "builtin/claude",
        ChannelCredential::Builtin(BuiltinChannelCredential::AiStudio(_)) => "builtin/aistudio",
        ChannelCredential::Builtin(BuiltinChannelCredential::VertexExpress(_)) => {
            "builtin/vertexexpress"
        }
        ChannelCredential::Builtin(BuiltinChannelCredential::Vertex(_)) => "builtin/vertex",
        ChannelCredential::Builtin(BuiltinChannelCredential::GeminiCli(_)) => "builtin/geminicli",
        ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(_)) => "builtin/claudecode",
        ChannelCredential::Builtin(BuiltinChannelCredential::Codex(_)) => "builtin/codex",
        ChannelCredential::Builtin(BuiltinChannelCredential::Antigravity(_)) => {
            "builtin/antigravity"
        }
        ChannelCredential::Builtin(BuiltinChannelCredential::Nvidia(_)) => "builtin/nvidia",
        ChannelCredential::Builtin(BuiltinChannelCredential::Deepseek(_)) => "builtin/deepseek",
        ChannelCredential::Custom(_) => "custom/apikey",
    }
    .to_string()
}

fn split_export_credential(
    credential: ChannelCredential,
) -> (Option<String>, Option<BuiltinChannelCredential>) {
    match credential {
        ChannelCredential::Custom(value) => (Some(value.api_key), None),
        ChannelCredential::Builtin(value) => match value {
            BuiltinChannelCredential::OpenAi(item) => (Some(item.api_key), None),
            BuiltinChannelCredential::Claude(item) => (Some(item.api_key), None),
            BuiltinChannelCredential::AiStudio(item) => (Some(item.api_key), None),
            BuiltinChannelCredential::VertexExpress(item) => (Some(item.api_key), None),
            BuiltinChannelCredential::Nvidia(item) => (Some(item.api_key), None),
            BuiltinChannelCredential::Deepseek(item) => (Some(item.api_key), None),
            other => (None, Some(other)),
        },
    }
}

fn export_credential_state(
    row: &gproxy_storage::CredentialStatusQueryRow,
) -> ExportCredentialState {
    let health = match parse_credential_health_from_status_row(row) {
        CredentialHealth::Healthy => ExportCredentialHealth::Healthy,
        CredentialHealth::Partial { models } => ExportCredentialHealth::Partial { models },
        CredentialHealth::Dead => ExportCredentialHealth::Dead,
    };
    let checked_at_unix_ms = row.checked_at.and_then(|value| {
        let unix_ms = value.unix_timestamp_nanos() / 1_000_000;
        if unix_ms < 0 {
            return None;
        }
        u64::try_from(unix_ms).ok()
    });
    ExportCredentialState {
        health,
        checked_at_unix_ms,
        last_error: row.last_error.clone(),
    }
}

fn parse_credential_health_from_status_row(
    row: &gproxy_storage::CredentialStatusQueryRow,
) -> CredentialHealth {
    match row.health_kind.as_str() {
        "healthy" => CredentialHealth::Healthy,
        "dead" => CredentialHealth::Dead,
        "partial" => {
            let models = if let Some(value) = row.health_json.clone() {
                if let Ok(models) = serde_json::from_value::<Vec<ModelCooldown>>(value.clone()) {
                    models
                } else {
                    value
                        .get("models")
                        .and_then(|item| {
                            serde_json::from_value::<Vec<ModelCooldown>>(item.clone()).ok()
                        })
                        .unwrap_or_default()
                }
            } else {
                Vec::new()
            };
            CredentialHealth::Partial { models }
        }
        _ => CredentialHealth::Healthy,
    }
}
