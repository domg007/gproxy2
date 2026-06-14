//! Cross-target admin/portal HTTP dispatcher.
//!
//! This module is the architecture-proving core for serving `/admin/*` and
//! `/user/*` on the edge (wasm) worker. It is gated `cfg(any(wasm32, test))`:
//! it compiles into the wasm edge build (which calls [`dispatch`] from
//! `edge/mod.rs::fetch`) AND into native **test** builds (so the dispatcher can
//! be driven by native integration tests), but is skipped in native release
//! builds to avoid dead code there — the native server has its own axum router.
//!
//! Every handler returns PURE DATA ([`Resp`], no `web_sys`), so a native test
//! can assert on `(status, body)` directly. The wasm `edge/mod.rs` converts the
//! returned `Resp` into a `web_sys::Response`.

pub mod crud;

use bytes::Bytes;
use http::request::Parts;
use http::{HeaderName, HeaderValue, Method, StatusCode};
use serde::de::DeserializeOwned;

use crate::admin::guard::{guard_admin, guard_session};
use crate::api::error::ApiError;
use crate::app::AppState;

/// Pure HTTP response data (no `web_sys`) so the dispatcher is natively
/// testable. The wasm edge converts this into a `web_sys::Response`.
#[derive(Debug)]
pub struct Resp {
    pub status: StatusCode,
    pub headers: Vec<(HeaderName, HeaderValue)>,
    pub body: Vec<u8>,
}

impl Resp {
    /// JSON response with the given status and `content-type: application/json`.
    pub(crate) fn json(status: u16, value: &impl serde::Serialize) -> Result<Resp, ApiError> {
        let body = serde_json::to_vec(value).map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok(Resp {
            status: StatusCode::from_u16(status).expect("caller passes a valid status"),
            headers: vec![(
                http::header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            body,
        })
    }

    /// Empty `204 No Content` (delete success), matching native CRUD semantics.
    pub(crate) fn no_content() -> Resp {
        Resp {
            status: StatusCode::NO_CONTENT,
            headers: vec![],
            body: vec![],
        }
    }
}

// ── Pure parse helpers (cross-target; the web_sys builders live in edge/http) ──

/// Split a URI path into non-empty segments: `/a/b/c` → `["a", "b", "c"]`.
pub(crate) fn segments(parts: &Parts) -> Vec<&str> {
    parts
        .uri
        .path()
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse a path segment as `i64`, mapping failures to [`ApiError::BadRequest`].
pub(crate) fn parse_i64(seg: &str) -> Result<i64, ApiError> {
    seg.parse::<i64>()
        .map_err(|_| ApiError::BadRequest(format!("invalid id: {seg}")))
}

/// Deserialize a JSON request body, mapping errors to [`ApiError::BadRequest`].
pub(crate) fn json_body<T: DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body)
        .map_err(|e| ApiError::BadRequest(format!("invalid JSON body: {e}")))
}

/// Deserialize URL-encoded query params from `parts.uri.query()`. An absent
/// query is treated as empty. Now cross-target: `serde_urlencoded` is in the
/// top-level `[dependencies]` (not wasm-gated), enabling this helper in both
/// edge and native test builds.
#[allow(dead_code)]
pub(crate) fn query<T: DeserializeOwned>(parts: &Parts) -> Result<T, ApiError> {
    serde_urlencoded::from_str(parts.uri.query().unwrap_or(""))
        .map_err(|e| ApiError::BadRequest(format!("invalid query: {e}")))
}

/// Map a persistence error to a 500 (the cause is logged, not leaked).
pub(crate) fn internal<E: std::fmt::Display>(e: E) -> ApiError {
    ApiError::Internal(e.to_string())
}

// ── Dispatcher ────────────────────────────────────────────────────────────────

/// Route an `/admin/*` or `/user/*` request to its handler.
///
/// Returns `Some(result)` when the path is a route we handle; `None` to fall
/// through (caller renders 404). Each handler runs its auth guard first, then
/// the logic, returning `Result<Resp, ApiError>` as pure data.
pub async fn dispatch(
    state: &AppState,
    parts: &Parts,
    body: &Bytes,
) -> Option<Result<Resp, ApiError>> {
    // 1. Try CRUD entities (providers/routes/aliases/rule-sets/orgs).
    if let Some(r) = crud::dispatch(state, parts, body).await {
        return Some(r);
    }

    // 2. Identity endpoints.
    let segs = segments(parts);
    let r = match (&parts.method, segs.as_slice()) {
        (&Method::GET, ["admin", "me"]) => admin_me(state, parts).await,
        (&Method::GET, ["user", "me"]) => user_me(state, parts).await,
        _ => return None,
    };
    Some(r)
}

// ── Handlers (auth guard first, then logic) ───────────────────────────────────

/// `GET /admin/me` — the authenticated admin identity.
async fn admin_me(state: &AppState, parts: &Parts) -> Result<Resp, ApiError> {
    let u = guard_admin(state, parts).await?;
    Resp::json(
        200,
        &serde_json::json!({ "id": u.id, "name": u.name, "is_admin": true }),
    )
}

/// `GET /user/me` — the portal session identity (admits any enabled user).
async fn user_me(state: &AppState, parts: &Parts) -> Result<Resp, ApiError> {
    let u = guard_session(state, parts).await?;
    Resp::json(
        200,
        &serde_json::json!({
            "id": u.id,
            "name": u.name,
            "is_admin": u.is_admin,
            "org_id": u.org_id,
            "team_id": u.team_id,
        }),
    )
}

#[cfg(test)]
mod tests;
