use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tokio::select;

use gproxy_core::state::AppState;
use gproxy_storage::Storage;

#[derive(Clone)]
pub struct AdminState {
    pub app: Arc<AppState>,
    pub storage: Arc<dyn Storage>,
}

pub fn router(app: Arc<AppState>, storage: Arc<dyn Storage>) -> Router {
    let state = AdminState { app, storage };

    Router::new()
        .route("/health", get(health))
        .route("/global", get(get_global))
        .route("/providers", get(list_providers))
        .route(
            "/providers/{name}/credentials",
            get(list_provider_credentials),
        )
        .route("/events/ws", get(events_ws))
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
    }))
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

    let creds: Vec<_> = snapshot
        .credentials
        .iter()
        .filter(|c| c.provider_id == provider.id)
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "name": c.name,
                "enabled": c.enabled,
                "created_at": c.created_at,
                "updated_at": c.updated_at,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({ "credentials": creds })),
    )
        .into_response()
}

async fn events_ws(ws: WebSocketUpgrade, State(state): State<AdminState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_events_ws(socket, state.app.clone()))
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
                if let Ok(text) = serde_json::to_string(&evt) &&
                    socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    }
}
