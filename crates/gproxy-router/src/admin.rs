use std::sync::Arc;
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs, io::Read, path::PathBuf, time::Duration};

use axum::Json;
use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use serde::Deserialize;
use time::{Duration as TimeDuration, OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::select;

use gproxy_core::state::{AppState, CredentialInsertInput, ProviderRuntime};
use gproxy_provider_core::{Credential, CredentialState, ProviderConfig, UnavailableReason};
use gproxy_storage::Storage;

#[derive(Clone)]
pub struct AdminState {
    pub app: Arc<AppState>,
    pub storage: Arc<dyn Storage>,
}

pub fn admin_router(app: Arc<AppState>, storage: Arc<dyn Storage>) -> Router {
    let state = AdminState { app, storage };

    Router::new()
        .route("/health", get(health))
        .route("/global_config", get(get_global).put(put_global))
        .route("/providers", get(list_providers))
        .route(
            "/providers/{name}",
            get(get_provider)
                .put(upsert_provider)
                .delete(delete_provider),
        )
        .route(
            "/providers/{name}/credentials",
            get(list_provider_credentials).post(insert_credential),
        )
        .route("/credentials/{id}/enabled", put(set_credential_enabled))
        .route(
            "/credentials/{id}",
            put(update_credential).delete(delete_credential),
        )
        .route("/credentials", get(list_credentials))
        .route(
            "/usage/providers/{provider}/tokens",
            get(usage_tokens_by_provider),
        )
        .route(
            "/usage/providers/{provider}/models/{model}/tokens",
            get(usage_tokens_by_provider_model),
        )
        .route(
            "/usage/credentials/{credential_id}/tokens",
            get(usage_tokens_by_credential),
        )
        .route(
            "/usage/credentials/{credential_id}/models/{model}/tokens",
            get(usage_tokens_by_credential_model),
        )
        .route("/logs", get(query_logs))
        .route("/users", get(list_users))
        .route("/users/{id}", put(upsert_user).delete(delete_user))
        .route("/users/{id}/enabled", put(set_user_enabled))
        .route(
            "/users/{id}/keys",
            post(insert_user_key).get(list_user_keys),
        )
        .route("/user_keys/{id}/enabled", put(set_user_key_enabled))
        .route(
            "/user_keys/{id}",
            put(update_user_key).delete(delete_user_key),
        )
        .route("/events/ws", get(events_ws))
        .route("/system/self_update", post(system_self_update))
        .layer(middleware::from_fn_with_state(state.clone(), admin_auth))
        .with_state(state)
}

async fn admin_auth(
    State(state): State<AdminState>,
    headers: HeaderMap,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let key = extract_admin_key(&headers, req.uri()).ok_or(StatusCode::UNAUTHORIZED)?;
    let expected_key = state.app.global.load().admin_key.clone();
    if key != expected_key {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}

fn extract_admin_key(headers: &HeaderMap, uri: &axum::http::Uri) -> Option<String> {
    if let Some(value) = headers.get("x-admin-key")
        && let Ok(s) = value.to_str()
    {
        let s = s.trim();
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }

    if let Some(value) = headers.get(header::AUTHORIZATION)
        && let Ok(auth) = value.to_str()
    {
        let auth = auth.trim();
        let prefix = "Bearer ";
        if auth.len() > prefix.len() && auth[..prefix.len()].eq_ignore_ascii_case(prefix) {
            let token = auth[prefix.len()..].trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    let query = uri.query()?;
    let parsed: std::collections::HashMap<String, String> =
        serde_urlencoded::from_str(query).ok()?;
    let key = parsed.get("admin_key")?.trim();
    if key.is_empty() {
        return None;
    }
    Some(key.to_string())
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
}

async fn get_global(State(state): State<AdminState>) -> impl IntoResponse {
    let global = state.app.global.load();
    Json(serde_json::json!({
        "host": global.host,
        "port": global.port,
        "admin_key": global.admin_key,
        "proxy": global.proxy,
        "dsn": global.dsn,
        "event_redact_sensitive": global.event_redact_sensitive,
    }))
}

#[derive(Debug, Deserialize)]
struct PutGlobalBody {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub admin_key: Option<String>,
    pub proxy: Option<String>,
    pub event_redact_sensitive: Option<bool>,
}

async fn put_global(
    State(state): State<AdminState>,
    Json(body): Json<PutGlobalBody>,
) -> impl IntoResponse {
    let patch = gproxy_common::GlobalConfigPatch {
        host: body.host,
        port: body.port,
        admin_key: body.admin_key.and_then(|key| {
            let trimmed = key.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }),
        proxy: body.proxy,
        dsn: None,
        event_redact_sensitive: body.event_redact_sensitive,
    };

    // DB commit -> in-memory apply (strong consistency).
    let current = state.app.global.load().as_ref().clone();
    let mut merged = gproxy_common::GlobalConfigPatch::from(current);
    merged.overlay(patch);
    let next = match merged.into_config() {
        Ok(v) => v,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid_global_config", "detail": err.to_string() })),
            )
                .into_response();
        }
    };

    if let Err(err) = state.storage.upsert_global_config(&next).await {
        return storage_error(err).into_response();
    }
    state.app.apply_global_config(next);

    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn list_providers(State(state): State<AdminState>) -> impl IntoResponse {
    let snapshot = state.app.snapshot.load();
    let providers: Vec<_> = snapshot
        .providers
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "enabled": p.enabled,
                "updated_at": p.updated_at,
            })
        })
        .collect();
    Json(serde_json::json!({ "providers": providers }))
}

async fn get_provider(
    State(state): State<AdminState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let snapshot = state.app.snapshot.load();
    let Some(p) = snapshot.providers.iter().find(|p| p.name == name) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "provider_not_found" })),
        )
            .into_response();
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "id": p.id,
            "name": p.name,
            "enabled": p.enabled,
            "config_json": p.config_json,
            "updated_at": p.updated_at,
        })),
    )
        .into_response()
}

fn unavailable_reason_code(reason: UnavailableReason) -> &'static str {
    match reason {
        UnavailableReason::RateLimit => "rate_limit",
        UnavailableReason::Timeout => "timeout",
        UnavailableReason::Upstream5xx => "upstream_5xx",
        UnavailableReason::AuthInvalid => "auth_invalid",
        UnavailableReason::ModelDisallow => "model_disallow",
        UnavailableReason::Manual => "manual",
        UnavailableReason::Unknown => "unknown",
    }
}

fn instant_remaining(until: tokio::time::Instant) -> Option<std::time::Duration> {
    let now = tokio::time::Instant::now();
    until.checked_duration_since(now)
}

fn until_epoch_millis(until: tokio::time::Instant) -> Option<i64> {
    let remaining = instant_remaining(until)?;
    let wall = SystemTime::now().checked_add(remaining)?;
    let millis = wall.duration_since(UNIX_EPOCH).ok()?.as_millis();
    i64::try_from(millis).ok()
}

async fn build_runtime_status(
    runtime: Option<&Arc<ProviderRuntime>>,
    credential_id: i64,
    enabled: bool,
) -> serde_json::Value {
    let Some(runtime) = runtime else {
        return serde_json::json!({
            "summary": if enabled { "active" } else { "disabled" },
            "credential_unavailable": serde_json::Value::Null,
            "model_unavailable": [],
        });
    };

    let credential_unavailable = match runtime.pool.state(credential_id).await {
        Some(CredentialState::Unavailable { until, reason }) => {
            instant_remaining(until).map(|remaining| {
                serde_json::json!({
                    "reason": unavailable_reason_code(reason),
                    "remaining_secs": remaining.as_secs(),
                    "remaining_ms": remaining.as_millis(),
                    "until_epoch_ms": until_epoch_millis(until),
                })
            })
        }
        _ => None,
    };

    let model_unavailable_rows = runtime
        .pool
        .model_states(credential_id)
        .await
        .into_iter()
        .filter_map(|(model, until, reason)| {
            let remaining = instant_remaining(until)?;
            Some(serde_json::json!({
                "model": model,
                "reason": unavailable_reason_code(reason),
                "remaining_secs": remaining.as_secs(),
                "remaining_ms": remaining.as_millis(),
                "until_epoch_ms": until_epoch_millis(until),
            }))
        })
        .collect::<Vec<_>>();

    let summary = if !enabled {
        "disabled"
    } else if credential_unavailable.is_some() {
        "fully_unavailable"
    } else if !model_unavailable_rows.is_empty() {
        "partial_unavailable"
    } else {
        "active"
    };

    serde_json::json!({
        "summary": summary,
        "credential_unavailable": credential_unavailable,
        "model_unavailable": model_unavailable_rows,
    })
}

async fn list_provider_credentials(
    State(state): State<AdminState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let snapshot = state.app.snapshot.load();
    let provider = snapshot.providers.iter().find(|p| p.name == name);
    let Some(provider) = provider else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "provider_not_found" })),
        )
            .into_response();
    };

    let runtime = state.app.providers.load().get(&name).cloned();
    let mut creds = Vec::new();
    for c in snapshot
        .credentials
        .iter()
        .filter(|c| c.provider_id == provider.id)
    {
        let runtime_status = build_runtime_status(runtime.as_ref(), c.id, c.enabled).await;
        creds.push(serde_json::json!({
            "id": c.id,
            "name": c.name,
            "settings_json": c.settings_json,
            "secret_json": c.secret_json,
            "enabled": c.enabled,
            "created_at": c.created_at,
            "updated_at": c.updated_at,
            "runtime_status": runtime_status,
        }));
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "credentials": creds })),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct UpsertProviderBody {
    pub enabled: bool,
    pub config_json: serde_json::Value,
}

async fn upsert_provider(
    State(state): State<AdminState>,
    Path(name): Path<String>,
    Json(body): Json<UpsertProviderBody>,
) -> impl IntoResponse {
    let id = match state
        .storage
        .upsert_provider(&name, &body.config_json, body.enabled)
        .await
    {
        Ok(id) => id,
        Err(err) => return storage_error(err).into_response(),
    };

    state
        .app
        .apply_provider_upsert(id, name.clone(), body.config_json, body.enabled);

    (
        StatusCode::OK,
        Json(serde_json::json!({ "id": id, "name": name })),
    )
        .into_response()
}

async fn delete_provider(
    State(state): State<AdminState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    // Only allow deleting custom providers. Builtin/bulletin providers must be disabled instead.
    let snapshot = state.app.snapshot.load();
    let provider = snapshot.providers.iter().find(|p| p.name == name);
    let Some(provider) = provider else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "provider_not_found" })),
        )
            .into_response();
    };
    let cfg: ProviderConfig = match serde_json::from_value(provider.config_json.clone()) {
        Ok(v) => v,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "provider_config_invalid", "detail": err.to_string() })),
            )
                .into_response();
        }
    };
    if !matches!(cfg, ProviderConfig::Custom(_)) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "only_custom_provider_can_be_deleted" })),
        )
            .into_response();
    }

    if let Err(err) = state.storage.delete_provider(&name).await {
        return storage_error(err).into_response();
    }
    state.app.apply_provider_delete(&name);
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

#[derive(Debug, Deserialize)]
struct InsertCredentialBody {
    pub name: Option<String>,
    #[serde(default = "default_object")]
    pub settings_json: serde_json::Value,
    pub secret_json: serde_json::Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

fn default_object() -> serde_json::Value {
    serde_json::json!({})
}

async fn insert_credential(
    State(state): State<AdminState>,
    Path(provider_name): Path<String>,
    Json(body): Json<InsertCredentialBody>,
) -> impl IntoResponse {
    // Validate provider exists in memory (snapshot + runtime map).
    let snapshot = state.app.snapshot.load();
    let provider = snapshot.providers.iter().find(|p| p.name == provider_name);
    let Some(provider) = provider else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "provider_not_found" })),
        )
            .into_response();
    };

    // Validate secret_json is a known Credential variant and matches provider config kind.
    let cred: Credential = match serde_json::from_value(body.secret_json.clone()) {
        Ok(c) => c,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_credential_json",
                    "detail": err.to_string(),
                })),
            )
                .into_response();
        }
    };

    // Parse provider config to enforce credential kind (best-effort).
    let runtime = state.app.providers.load().get(&provider_name).cloned();
    if let Some(runtime) = runtime
        && let Ok(cfg) =
            serde_json::from_value::<ProviderConfig>(runtime.config_json.load().as_ref().clone())
        && !credential_matches_provider(&cred, &cfg)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "credential_kind_mismatch" })),
        )
            .into_response();
    }

    let id = match state
        .storage
        .insert_credential(
            &provider_name,
            body.name.as_deref(),
            &body.settings_json,
            &body.secret_json,
            body.enabled,
        )
        .await
    {
        Ok(id) => id,
        Err(err) => return storage_error(err).into_response(),
    };

    if let Err(err) = state
        .app
        .apply_credential_insert(CredentialInsertInput {
            id,
            provider_name,
            provider_id: provider.id,
            name: body.name,
            settings_json: body.settings_json,
            secret_json: body.secret_json,
            enabled: body.enabled,
        })
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "apply_memory_failed", "detail": err.to_string() })),
        )
            .into_response();
    }

    (StatusCode::OK, Json(serde_json::json!({ "id": id }))).into_response()
}

#[derive(Debug, Deserialize)]
struct SetEnabledBody {
    pub enabled: bool,
}

async fn set_credential_enabled(
    State(state): State<AdminState>,
    Path(id): Path<i64>,
    Json(body): Json<SetEnabledBody>,
) -> impl IntoResponse {
    if let Err(err) = state.storage.set_credential_enabled(id, body.enabled).await {
        return storage_error(err).into_response();
    }

    if let Err(err) = state.app.apply_credential_enabled(id, body.enabled).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "apply_memory_failed", "detail": err.to_string() })),
        )
            .into_response();
    }

    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn delete_credential(
    State(state): State<AdminState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    // Ensure it won't be acquired anymore after deletion.
    {
        let snapshot = state.app.snapshot.load();
        if let Some(row) = snapshot.credentials.iter().find(|c| c.id == id)
            && let Some(provider_name) = snapshot
                .providers
                .iter()
                .find(|p| p.id == row.provider_id)
                .map(|p| p.name.clone())
            && let Some(runtime) = state.app.providers.load().get(&provider_name).cloned()
        {
            runtime.pool.set_enabled(&provider_name, id, false).await;
        }
    }

    if let Err(err) = state.storage.delete_credential(id).await {
        return storage_error(err).into_response();
    }

    // Best-effort: remove from snapshot. Pool removal is handled by disabling before delete.
    state.app.apply_credential_delete(id);

    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

#[derive(Debug, Deserialize)]
struct UpdateCredentialBody {
    pub name: Option<String>,
    pub settings_json: Option<serde_json::Value>,
    pub secret_json: serde_json::Value,
}

async fn update_credential(
    State(state): State<AdminState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateCredentialBody>,
) -> impl IntoResponse {
    // Validate secret_json is a known Credential variant.
    let cred: Credential = match serde_json::from_value(body.secret_json.clone()) {
        Ok(c) => c,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_credential_json",
                    "detail": err.to_string(),
                })),
            )
                .into_response();
        }
    };

    // Validate provider kind matches existing provider config kind.
    let snapshot = state.app.snapshot.load();
    let existing = snapshot.credentials.iter().find(|c| c.id == id);
    let Some(existing) = existing else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "credential_not_found" })),
        )
            .into_response();
    };
    let provider_name = snapshot
        .providers
        .iter()
        .find(|p| p.id == existing.provider_id)
        .map(|p| p.name.clone());
    let Some(provider_name) = provider_name else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "provider_not_found" })),
        )
            .into_response();
    };
    let runtime = state.app.providers.load().get(&provider_name).cloned();
    if let Some(runtime) = runtime
        && let Ok(cfg) =
            serde_json::from_value::<ProviderConfig>(runtime.config_json.load().as_ref().clone())
        && !credential_matches_provider(&cred, &cfg)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "credential_kind_mismatch" })),
        )
            .into_response();
    }

    if let Err(err) = state
        .storage
        .update_credential(
            id,
            body.name.as_deref(),
            body.settings_json
                .as_ref()
                .unwrap_or(&existing.settings_json),
            &body.secret_json,
        )
        .await
    {
        return storage_error(err).into_response();
    }

    if let Err(err) = state
        .app
        .apply_credential_update(
            id,
            body.name,
            body.settings_json.unwrap_or(existing.settings_json.clone()),
            body.secret_json,
        )
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "apply_memory_failed", "detail": err.to_string() })),
        )
            .into_response();
    }

    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn list_credentials(State(state): State<AdminState>) -> impl IntoResponse {
    let snapshot = state.app.snapshot.load();
    let provider_map: std::collections::HashMap<i64, String> = snapshot
        .providers
        .iter()
        .map(|p| (p.id, p.name.clone()))
        .collect();
    let runtime_map = state.app.providers.load();
    let mut creds = Vec::new();
    for c in &snapshot.credentials {
        let runtime = provider_map
            .get(&c.provider_id)
            .and_then(|provider_name| runtime_map.get(provider_name).cloned());
        let runtime_status = build_runtime_status(runtime.as_ref(), c.id, c.enabled).await;
        creds.push(serde_json::json!({
            "id": c.id,
            "provider_id": c.provider_id,
            "name": c.name,
            "settings_json": c.settings_json,
            "secret_json": c.secret_json,
            "enabled": c.enabled,
            "created_at": c.created_at,
            "updated_at": c.updated_at,
            "runtime_status": runtime_status,
        }));
    }
    Json(serde_json::json!({ "credentials": creds }))
}

#[derive(Debug, Deserialize)]
struct UsageRangeQuery {
    from: String,
    to: String,
    #[serde(default)]
    model_contains: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    credential_id: Option<i64>,
    #[serde(default)]
    user_id: Option<i64>,
    #[serde(default)]
    user_key_id: Option<i64>,
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    operation: Option<String>,
    #[serde(default)]
    path_contains: Option<String>,
    #[serde(default)]
    status_min: Option<i32>,
    #[serde(default)]
    status_max: Option<i32>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
}

async fn usage_tokens_by_provider(
    State(state): State<AdminState>,
    Path(provider): Path<String>,
    Query(query): Query<UsageRangeQuery>,
) -> impl IntoResponse {
    let (from, to) = match parse_usage_range(&query) {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };

    let aggregate = match state
        .storage
        .aggregate_usage_tokens(gproxy_storage::UsageAggregateFilter {
            from,
            to,
            provider: Some(provider.clone()),
            credential_id: None,
            model: None,
            model_contains: query.model_contains.clone(),
        })
        .await
    {
        Ok(v) => v,
        Err(err) => return storage_error(err).into_response(),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "scope": "provider",
            "provider": provider,
            "from": query.from,
            "to": query.to,
            "matched_rows": aggregate.matched_rows,
            "call_count": aggregate.matched_rows,
            "input_tokens": aggregate.input_tokens,
            "output_tokens": aggregate.output_tokens,
            "cache_read_input_tokens": aggregate.cache_read_input_tokens,
            "cache_creation_input_tokens": aggregate.cache_creation_input_tokens,
            "total_tokens": aggregate.total_tokens,
        })),
    )
        .into_response()
}

async fn usage_tokens_by_provider_model(
    State(state): State<AdminState>,
    Path((provider, model)): Path<(String, String)>,
    Query(query): Query<UsageRangeQuery>,
) -> impl IntoResponse {
    let (from, to) = match parse_usage_range(&query) {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };

    let aggregate = match state
        .storage
        .aggregate_usage_tokens(gproxy_storage::UsageAggregateFilter {
            from,
            to,
            provider: Some(provider.clone()),
            credential_id: None,
            model: Some(model.clone()),
            model_contains: query.model_contains.clone(),
        })
        .await
    {
        Ok(v) => v,
        Err(err) => return storage_error(err).into_response(),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "scope": "provider_model",
            "provider": provider,
            "model": model,
            "from": query.from,
            "to": query.to,
            "matched_rows": aggregate.matched_rows,
            "call_count": aggregate.matched_rows,
            "input_tokens": aggregate.input_tokens,
            "output_tokens": aggregate.output_tokens,
            "cache_read_input_tokens": aggregate.cache_read_input_tokens,
            "cache_creation_input_tokens": aggregate.cache_creation_input_tokens,
            "total_tokens": aggregate.total_tokens,
        })),
    )
        .into_response()
}

async fn usage_tokens_by_credential(
    State(state): State<AdminState>,
    Path(credential_id): Path<i64>,
    Query(query): Query<UsageRangeQuery>,
) -> impl IntoResponse {
    let (from, to) = match parse_usage_range(&query) {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };

    let aggregate = match state
        .storage
        .aggregate_usage_tokens(gproxy_storage::UsageAggregateFilter {
            from,
            to,
            provider: None,
            credential_id: Some(credential_id),
            model: None,
            model_contains: query.model_contains.clone(),
        })
        .await
    {
        Ok(v) => v,
        Err(err) => return storage_error(err).into_response(),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "scope": "credential",
            "credential_id": credential_id,
            "from": query.from,
            "to": query.to,
            "matched_rows": aggregate.matched_rows,
            "call_count": aggregate.matched_rows,
            "input_tokens": aggregate.input_tokens,
            "output_tokens": aggregate.output_tokens,
            "cache_read_input_tokens": aggregate.cache_read_input_tokens,
            "cache_creation_input_tokens": aggregate.cache_creation_input_tokens,
            "total_tokens": aggregate.total_tokens,
        })),
    )
        .into_response()
}

async fn usage_tokens_by_credential_model(
    State(state): State<AdminState>,
    Path((credential_id, model)): Path<(i64, String)>,
    Query(query): Query<UsageRangeQuery>,
) -> impl IntoResponse {
    let (from, to) = match parse_usage_range(&query) {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };

    let aggregate = match state
        .storage
        .aggregate_usage_tokens(gproxy_storage::UsageAggregateFilter {
            from,
            to,
            provider: None,
            credential_id: Some(credential_id),
            model: Some(model.clone()),
            model_contains: query.model_contains.clone(),
        })
        .await
    {
        Ok(v) => v,
        Err(err) => return storage_error(err).into_response(),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "scope": "credential_model",
            "credential_id": credential_id,
            "model": model,
            "from": query.from,
            "to": query.to,
            "matched_rows": aggregate.matched_rows,
            "call_count": aggregate.matched_rows,
            "input_tokens": aggregate.input_tokens,
            "output_tokens": aggregate.output_tokens,
            "cache_read_input_tokens": aggregate.cache_read_input_tokens,
            "cache_creation_input_tokens": aggregate.cache_creation_input_tokens,
            "total_tokens": aggregate.total_tokens,
        })),
    )
        .into_response()
}

async fn query_logs(State(state): State<AdminState>, Query(query): Query<LogsQuery>) -> impl IntoResponse {
    let kind = match normalize_opt_str(query.kind).as_deref() {
        None | Some("all") => None,
        Some("upstream") => Some(gproxy_storage::LogRecordKind::Upstream),
        Some("downstream") => Some(gproxy_storage::LogRecordKind::Downstream),
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_kind",
                    "detail": format!("unsupported kind: {other}; expected one of all/upstream/downstream"),
                })),
            )
                .into_response();
        }
    };

    if let (Some(status_min), Some(status_max)) = (query.status_min, query.status_max)
        && status_max < status_min
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_status_range",
                "detail": "`status_max` must be >= `status_min`",
            })),
        )
            .into_response();
    }

    let now = OffsetDateTime::now_utc();
    let default_from = now - TimeDuration::hours(24);
    let from = match normalize_opt_str(query.from) {
        Some(raw) => match OffsetDateTime::parse(&raw, &Rfc3339) {
            Ok(v) => v,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "invalid_from",
                        "detail": err.to_string(),
                    })),
                )
                    .into_response();
            }
        },
        None => default_from,
    };
    let to = match normalize_opt_str(query.to) {
        Some(raw) => match OffsetDateTime::parse(&raw, &Rfc3339) {
            Ok(v) => v,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "invalid_to",
                        "detail": err.to_string(),
                    })),
                )
                    .into_response();
            }
        },
        None => now,
    };
    if to < from {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_range",
                "detail": "`to` must be >= `from`",
            })),
        )
            .into_response();
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let offset = query.offset.unwrap_or(0);

    let filter = gproxy_storage::LogQueryFilter {
        from,
        to,
        kind,
        provider: normalize_opt_str(query.provider),
        credential_id: query.credential_id,
        user_id: query.user_id,
        user_key_id: query.user_key_id,
        trace_id: normalize_opt_str(query.trace_id),
        operation: normalize_opt_str(query.operation),
        request_path_contains: normalize_opt_str(query.path_contains),
        status_min: query.status_min,
        status_max: query.status_max,
        limit,
        offset,
    };

    let result = match state.storage.query_logs(filter).await {
        Ok(v) => v,
        Err(err) => return storage_error(err).into_response(),
    };

    let rows: Vec<_> = result
        .rows
        .into_iter()
        .map(|row| {
            let kind = match row.kind {
                gproxy_storage::LogRecordKind::Upstream => "upstream",
                gproxy_storage::LogRecordKind::Downstream => "downstream",
            };
            serde_json::json!({
                "id": row.id,
                "kind": kind,
                "at": format_time_rfc3339(row.at),
                "trace_id": row.trace_id,
                "provider": row.provider,
                "credential_id": row.credential_id,
                "user_id": row.user_id,
                "user_key_id": row.user_key_id,
                "attempt_no": row.attempt_no,
                "operation": row.operation,
                "request_method": row.request_method,
                "request_path": row.request_path,
                "response_status": row.response_status,
                "error_kind": row.error_kind,
                "error_message": row.error_message,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "from": format_time_rfc3339(from),
            "to": format_time_rfc3339(to),
            "kind": match kind {
                None => "all",
                Some(gproxy_storage::LogRecordKind::Upstream) => "upstream",
                Some(gproxy_storage::LogRecordKind::Downstream) => "downstream",
            },
            "limit": limit,
            "offset": offset,
            "has_more": result.has_more,
            "rows": rows,
        })),
    )
        .into_response()
}

async fn list_users(State(state): State<AdminState>) -> impl IntoResponse {
    let snapshot = state.app.snapshot.load();
    let users: Vec<_> = snapshot
        .users
        .iter()
        .map(|u| {
            serde_json::json!({
                "id": u.id,
                "name": u.name,
                "enabled": u.enabled,
                "created_at": u.created_at,
                "updated_at": u.updated_at,
            })
        })
        .collect();
    Json(serde_json::json!({ "users": users }))
}

#[derive(Debug, Deserialize)]
struct UpsertUserBody {
    pub name: String,
    pub enabled: bool,
}

async fn upsert_user(
    State(state): State<AdminState>,
    Path(id): Path<i64>,
    Json(body): Json<UpsertUserBody>,
) -> impl IntoResponse {
    if let Err(err) = state
        .storage
        .upsert_user_by_id(id, &body.name, body.enabled)
        .await
    {
        return storage_error(err).into_response();
    }
    state
        .app
        .apply_user_upsert(id, body.name.clone(), body.enabled);
    (
        StatusCode::OK,
        Json(serde_json::json!({ "id": id, "name": body.name })),
    )
        .into_response()
}

async fn delete_user(State(state): State<AdminState>, Path(id): Path<i64>) -> impl IntoResponse {
    if let Err(err) = state.storage.delete_user(id).await {
        return storage_error(err).into_response();
    }
    state.app.apply_user_delete(id);
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn set_user_enabled(
    State(state): State<AdminState>,
    Path(id): Path<i64>,
    Json(body): Json<SetEnabledBody>,
) -> impl IntoResponse {
    if let Err(err) = state.storage.set_user_enabled(id, body.enabled).await {
        return storage_error(err).into_response();
    }
    state.app.apply_user_enabled(id, body.enabled);
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

#[derive(Debug, Deserialize)]
struct InsertUserKeyBody {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

async fn insert_user_key(
    State(state): State<AdminState>,
    Path(user_id): Path<i64>,
    Json(body): Json<InsertUserKeyBody>,
) -> impl IntoResponse {
    let key_plain = body.key.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let id = match state
        .storage
        .insert_user_key(user_id, &key_plain, body.label.as_deref(), body.enabled)
        .await
    {
        Ok(id) => id,
        Err(err) => return storage_error(err).into_response(),
    };

    state
        .app
        .apply_user_key_insert(id, user_id, key_plain.clone(), body.label, body.enabled);

    (
        StatusCode::OK,
        Json(serde_json::json!({ "id": id, "key": key_plain })),
    )
        .into_response()
}

async fn list_user_keys(
    State(state): State<AdminState>,
    Path(user_id): Path<i64>,
) -> impl IntoResponse {
    let snapshot = state.app.snapshot.load();
    let keys: Vec<_> = snapshot
        .user_keys
        .iter()
        .filter(|k| k.user_id == user_id)
        .map(|k| {
            serde_json::json!({
                "id": k.id,
                "user_id": k.user_id,
                "label": k.label,
                "enabled": k.enabled,
                "created_at": k.created_at,
                "updated_at": k.updated_at,
            })
        })
        .collect();
    Json(serde_json::json!({ "keys": keys }))
}

async fn set_user_key_enabled(
    State(state): State<AdminState>,
    Path(id): Path<i64>,
    Json(body): Json<SetEnabledBody>,
) -> impl IntoResponse {
    if let Err(err) = state.storage.set_user_key_enabled(id, body.enabled).await {
        return storage_error(err).into_response();
    }
    state.app.apply_user_key_enabled(id, body.enabled);
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

#[derive(Debug, Deserialize)]
struct UpdateUserKeyBody {
    pub label: Option<String>,
}

async fn update_user_key(
    State(state): State<AdminState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateUserKeyBody>,
) -> impl IntoResponse {
    if let Err(err) = state
        .storage
        .update_user_key_label(id, body.label.as_deref())
        .await
    {
        return storage_error(err).into_response();
    }
    state.app.apply_user_key_label(id, body.label);
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn delete_user_key(
    State(state): State<AdminState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    if let Err(err) = state.storage.delete_user_key(id).await {
        return storage_error(err).into_response();
    }
    state.app.apply_user_key_delete(id);
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn events_ws(ws: WebSocketUpgrade, State(state): State<AdminState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_events_ws(socket, state.app.clone()))
}

const GPROXY_REPO_API_LATEST: &str = "https://api.github.com/repos/LeenHawk/gproxy/releases/latest";

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

async fn system_self_update(State(state): State<AdminState>) -> impl IntoResponse {
    let proxy = state.app.global.load().proxy.clone();
    match self_update_to_latest_release(proxy).await {
        Ok(result) => match schedule_self_restart() {
            Ok(()) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "from_version": env!("CARGO_PKG_VERSION"),
                    "release_tag": result.release_tag,
                    "asset": result.asset_name,
                    "installed_to": result.installed_to,
                    "restart_required": false,
                    "restart_scheduled": true,
                    "note": "Update prepared and process restart scheduled automatically."
                })),
            )
                .into_response(),
            Err(err) => (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "self_restart_schedule_failed",
                    "detail": err,
                    "release_tag": result.release_tag,
                    "asset": result.asset_name,
                    "installed_to": result.installed_to
                })),
            )
                .into_response(),
        },
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": "self_update_failed",
                "detail": err
            })),
        )
            .into_response(),
    }
}

struct SelfUpdateResult {
    release_tag: String,
    asset_name: String,
    installed_to: String,
}

#[cfg(windows)]
static WINDOWS_PENDING_SELF_UPDATE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

#[cfg(windows)]
fn windows_pending_self_update() -> &'static Mutex<Option<PathBuf>> {
    WINDOWS_PENDING_SELF_UPDATE.get_or_init(|| Mutex::new(None))
}

#[cfg(windows)]
fn set_windows_pending_self_update(path: PathBuf) -> Result<(), String> {
    let mut guard = windows_pending_self_update()
        .lock()
        .map_err(|_| "windows_pending_self_update_lock_failed".to_string())?;
    if let Some(prev) = guard.replace(path)
        && prev.exists()
    {
        let _ = fs::remove_file(prev);
    }
    Ok(())
}

#[cfg(windows)]
fn take_windows_pending_self_update() -> Option<PathBuf> {
    windows_pending_self_update().lock().ok()?.take()
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
        .map_err(|e| format!("fetch_latest_release: {e}"))?;
    if !release_resp.status().is_success() {
        let status = release_resp.status();
        let body = release_resp
            .bytes()
            .await
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_else(|_| String::new());
        return Err(format!("fetch_latest_release_status_{status}: {body}"));
    }
    let release_body = release_resp
        .bytes()
        .await
        .map_err(|e| format!("read_latest_release_body: {e}"))?;
    let release: GithubReleaseInfo = serde_json::from_slice(&release_body)
        .map_err(|e| format!("parse_latest_release_json: {e}"))?;
    let asset = release
        .assets
        .iter()
        .find(|item| item.name == target_asset)
        .cloned()
        .ok_or_else(|| {
            let names = release
                .assets
                .iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            format!("asset_not_found_for_target:{target_asset}; available=[{names}]")
        })?;
    Ok((release.tag_name, asset))
}

#[cfg(windows)]
async fn self_update_to_latest_release(proxy: Option<String>) -> Result<SelfUpdateResult, String> {
    let target_asset = target_release_asset_name()?;
    let client = build_self_update_client(proxy)?;

    let (release_tag, asset) = fetch_latest_release_asset(&client, &target_asset).await?;
    let zip_bytes = download_bytes_with_redirects(&client, &asset.browser_download_url, 8).await?;
    let binary_bytes = extract_binary_from_zip(&zip_bytes)?;
    let staged = stage_windows_binary_bytes(binary_bytes)?;
    set_windows_pending_self_update(staged.clone())?;

    let installed_to = env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());
    Ok(SelfUpdateResult {
        release_tag,
        asset_name: asset.name,
        installed_to,
    })
}

#[cfg(not(windows))]
async fn self_update_to_latest_release(proxy: Option<String>) -> Result<SelfUpdateResult, String> {
    let target_asset = target_release_asset_name()?;
    let client = build_self_update_client(proxy)?;

    let (release_tag, asset) = fetch_latest_release_asset(&client, &target_asset).await?;

    let zip_bytes = download_bytes_with_redirects(&client, &asset.browser_download_url, 8).await?;

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

fn build_self_update_client(proxy: Option<String>) -> Result<wreq::Client, String> {
    let proxy = proxy
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let mut builder = wreq::Client::builder();
    if let Some(proxy) = proxy.as_deref() {
        let parsed = wreq::Proxy::all(proxy).map_err(|e| format!("invalid_proxy:{e}"))?;
        builder = builder.proxy(parsed);
    }
    builder
        .build()
        .map_err(|e| format!("build_http_client: {e}"))
}

async fn download_bytes_with_redirects(
    client: &wreq::Client,
    url: &str,
    max_redirects: usize,
) -> Result<bytes::Bytes, String> {
    let mut current = url.to_string();

    for _ in 0..=max_redirects {
        let resp = client
            .get(&current)
            .header("accept", "application/octet-stream")
            .header("user-agent", concat!("gproxy/", env!("CARGO_PKG_VERSION")))
            .send()
            .await
            .map_err(|e| format!("download_asset:{current}:{e}"))?;

        let status = resp.status();
        if status.is_success() {
            return resp
                .bytes()
                .await
                .map_err(|e| format!("read_asset_body:{current}:{e}"));
        }

        if status.is_redirection() {
            let Some(location) = resp
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                return Err(format!("redirect_without_location:{status}:{current}"));
            };

            let next = if location.starts_with("http://") || location.starts_with("https://") {
                location.to_string()
            } else {
                return Err(format!(
                    "relative_redirect_unsupported:{status}:{current}:{location}"
                ));
            };
            current = next;
            continue;
        }

        let body = resp
            .bytes()
            .await
            .map(|b| String::from_utf8_lossy(&b).to_string())
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

fn extract_binary_from_zip(zip_bytes: &bytes::Bytes) -> Result<Vec<u8>, String> {
    let cursor = std::io::Cursor::new(zip_bytes.to_vec());
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("open_zip_archive: {e}"))?;

    let exe_name = if cfg!(windows) {
        "gproxy.exe"
    } else {
        "gproxy"
    };
    let mut file = archive
        .by_name(exe_name)
        .map_err(|e| format!("zip_entry_not_found:{exe_name}:{e}"))?;

    let mut out = Vec::new();
    file.read_to_end(&mut out)
        .map_err(|e| format!("read_zip_entry:{e}"))?;
    if out.is_empty() {
        return Err("zip_entry_empty".to_string());
    }
    Ok(out)
}

fn install_binary_bytes(binary: Vec<u8>) -> Result<(), String> {
    let current = env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let parent = current
        .parent()
        .ok_or_else(|| "current_exe_parent_missing".to_string())?;
    let temp = temp_update_path(parent);

    fs::write(&temp, &binary).map_err(|e| format!("write_temp_binary:{}:{e}", temp.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&current)
            .map(|m| m.permissions().mode())
            .unwrap_or(0o755);
        fs::set_permissions(&temp, fs::Permissions::from_mode(mode))
            .map_err(|e| format!("set_temp_permissions:{}:{e}", temp.display()))?;
    }

    fs::rename(&temp, &current).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!(
            "replace_binary_failed:{}->{}:{e}",
            temp.display(),
            current.display()
        )
    })?;

    Ok(())
}

#[cfg(windows)]
fn stage_windows_binary_bytes(binary: Vec<u8>) -> Result<PathBuf, String> {
    let current = env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let parent = current
        .parent()
        .ok_or_else(|| "current_exe_parent_missing".to_string())?;
    let temp = temp_update_path(parent);
    fs::write(&temp, &binary).map_err(|e| format!("write_staged_binary:{}:{e}", temp.display()))?;
    Ok(temp)
}

fn temp_update_path(parent: &std::path::Path) -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    if cfg!(windows) {
        parent.join(format!("gproxy-update-{pid}-{nanos}.exe.new"))
    } else {
        parent.join(format!(".gproxy-update-{pid}-{nanos}.new"))
    }
}

fn schedule_self_restart() -> Result<(), String> {
    let exe = env::current_exe().map_err(|e| format!("current_exe_for_restart: {e}"))?;
    let args: Vec<std::ffi::OsString> = env::args_os().skip(1).collect();
    #[cfg(windows)]
    let pending_update = take_windows_pending_self_update();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        #[cfg(windows)]
        restart_current_process(exe, args, pending_update);
        #[cfg(not(windows))]
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

#[cfg(windows)]
fn restart_current_process(
    exe: PathBuf,
    args: Vec<std::ffi::OsString>,
    pending_update: Option<PathBuf>,
) {
    if let Some(staged) = pending_update {
        let script = build_windows_self_update_script(&exe, &staged, &args);
        match std::process::Command::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-WindowStyle")
            .arg("Hidden")
            .arg("-Command")
            .arg(script)
            .spawn()
        {
            Ok(_) => std::process::exit(0),
            Err(err) => {
                eprintln!(
                    "self_update powershell spawn failed for {} with staged {}: {err}",
                    exe.display(),
                    staged.display()
                );
            }
        }
    }

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

#[cfg(all(not(unix), not(windows)))]
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

#[cfg(windows)]
fn build_windows_self_update_script(
    exe: &std::path::Path,
    staged: &std::path::Path,
    args: &[std::ffi::OsString],
) -> String {
    let exe_quoted = powershell_single_quote(&exe.to_string_lossy());
    let staged_quoted = powershell_single_quote(&staged.to_string_lossy());
    let args_array = if args.is_empty() {
        "@()".to_string()
    } else {
        let joined = args
            .iter()
            .map(|arg| format!("'{}'", powershell_single_quote(&arg.to_string_lossy())))
            .collect::<Vec<_>>()
            .join(", ");
        format!("@({joined})")
    };

    format!(
        "$ErrorActionPreference='SilentlyContinue'; \
         $exe='{exe_quoted}'; \
         $new='{staged_quoted}'; \
         $args={args_array}; \
         for ($i=0; $i -lt 120; $i++) {{ \
             try {{ Move-Item -LiteralPath $new -Destination $exe -Force; break }} \
             catch {{ Start-Sleep -Milliseconds 500 }} \
         }}; \
         Start-Process -FilePath $exe -ArgumentList $args"
    )
}

#[cfg(windows)]
fn powershell_single_quote(input: &str) -> String {
    input.replace('\'', "''")
}

async fn handle_events_ws(mut socket: WebSocket, app: Arc<AppState>) {
    let mut rx = app.events.subscribe();

    loop {
        select! {
            msg = socket.recv() => {
                // If peer disconnects or errors, stop.
                if msg.is_none() {
                    break;
                }
            }
            evt = rx.recv() => {
                let Ok(evt) = evt else {
                    break;
                };
                if let Ok(text) = evt.to_log_json()
                    && socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
            }
        }
    }
}

fn credential_matches_provider(cred: &Credential, cfg: &ProviderConfig) -> bool {
    use gproxy_provider_core::Credential as C;
    use gproxy_provider_core::ProviderConfig as P;

    matches!(
        (cred, cfg),
        (C::OpenAI(_), P::OpenAI(_))
            | (C::Claude(_), P::Claude(_))
            | (C::AIStudio(_), P::AIStudio(_))
            | (C::VertexExpress(_), P::VertexExpress(_))
            | (C::Vertex(_), P::Vertex(_))
            | (C::GeminiCli(_), P::GeminiCli(_))
            | (C::ClaudeCode(_), P::ClaudeCode(_))
            | (C::Codex(_), P::Codex(_))
            | (C::Antigravity(_), P::Antigravity(_))
            | (C::Nvidia(_), P::Nvidia(_))
            | (C::DeepSeek(_), P::DeepSeek(_))
            | (C::Custom(_), P::Custom(_))
    )
}

fn parse_usage_range(
    query: &UsageRangeQuery,
) -> Result<(OffsetDateTime, OffsetDateTime), (StatusCode, Json<serde_json::Value>)> {
    let from = match OffsetDateTime::parse(&query.from, &Rfc3339) {
        Ok(v) => v,
        Err(err) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_from",
                    "detail": err.to_string(),
                })),
            ));
        }
    };
    let to = match OffsetDateTime::parse(&query.to, &Rfc3339) {
        Ok(v) => v,
        Err(err) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_to",
                    "detail": err.to_string(),
                })),
            ));
        }
    };
    if to < from {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid_range",
                "detail": "`to` must be >= `from`",
            })),
        ));
    }
    Ok((from, to))
}

fn normalize_opt_str(input: Option<String>) -> Option<String> {
    input.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn format_time_rfc3339(value: OffsetDateTime) -> String {
    value
        .format(&Rfc3339)
        .unwrap_or_else(|_| value.unix_timestamp().to_string())
}

fn storage_error(err: gproxy_storage::StorageError) -> (StatusCode, Json<serde_json::Value>) {
    // TODO: map common unique constraint errors to 409.
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": "storage_error", "detail": err.to_string() })),
    )
}
