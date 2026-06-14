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

use bytes::Bytes;
use http::request::Parts;
use http::{HeaderName, HeaderValue, Method, StatusCode};
use serde::de::DeserializeOwned;

use crate::admin::guard::{guard_admin, guard_session};
use crate::admin::invalidate;
use crate::api::error::ApiError;
use crate::app::AppState;
use crate::store::persistence::records::OrgInput;

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
    fn json(status: u16, value: &impl serde::Serialize) -> Result<Resp, ApiError> {
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
    fn no_content() -> Resp {
        Resp {
            status: StatusCode::NO_CONTENT,
            headers: vec![],
            body: vec![],
        }
    }
}

// ── Pure parse helpers (cross-target; the web_sys builders live in edge/http) ──

/// Split a URI path into non-empty segments: `/a/b/c` → `["a", "b", "c"]`.
fn segments(parts: &Parts) -> Vec<&str> {
    parts
        .uri
        .path()
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse a path segment as `i64`, mapping failures to [`ApiError::BadRequest`].
fn parse_i64(seg: &str) -> Result<i64, ApiError> {
    seg.parse::<i64>()
        .map_err(|_| ApiError::BadRequest(format!("invalid id: {seg}")))
}

/// Deserialize a JSON request body, mapping errors to [`ApiError::BadRequest`].
fn json_body<T: DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body)
        .map_err(|e| ApiError::BadRequest(format!("invalid JSON body: {e}")))
}

/// Deserialize URL-encoded query params from `parts.uri.query()`. An absent
/// query is treated as empty. Kept here for the upcoming read endpoints (B6.2).
///
/// `serde_urlencoded` is a wasm-only dependency (it is declared under the
/// wasm target in `Cargo.toml`), so this helper is wasm-gated; B6.2's read
/// routes run on the edge worker, which is where it is needed.
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
fn query<T: DeserializeOwned>(parts: &Parts) -> Result<T, ApiError> {
    serde_urlencoded::from_str(parts.uri.query().unwrap_or(""))
        .map_err(|e| ApiError::BadRequest(format!("invalid query: {e}")))
}

/// Map a persistence error to a 500 (the cause is logged, not leaked).
fn internal<E: std::fmt::Display>(e: E) -> ApiError {
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
    let segs = segments(parts);
    let r = match (&parts.method, segs.as_slice()) {
        (&Method::GET, ["admin", "me"]) => admin_me(state, parts).await,
        (&Method::GET, ["admin", "orgs"]) => orgs_list(state, parts).await,
        (&Method::POST, ["admin", "orgs"]) => orgs_upsert(state, parts, body).await,
        (&Method::GET, ["admin", "orgs", id]) => orgs_get(state, parts, id).await,
        (&Method::DELETE, ["admin", "orgs", id]) => orgs_delete(state, parts, id).await,
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

/// `GET /admin/orgs` — list all orgs (thin glue over persistence).
async fn orgs_list(state: &AppState, parts: &Parts) -> Result<Resp, ApiError> {
    guard_admin(state, parts).await?;
    let orgs = state.persistence.list_orgs().await.map_err(internal)?;
    Resp::json(200, &orgs)
}

/// `POST /admin/orgs` — insert/update an org, then invalidate (no-op on edge).
async fn orgs_upsert(state: &AppState, parts: &Parts, body: &Bytes) -> Result<Resp, ApiError> {
    guard_admin(state, parts).await?;
    let input: OrgInput = json_body(body)?;
    let org = state
        .persistence
        .upsert_org(input)
        .await
        .map_err(ApiError::from_upsert)?;
    invalidate(state).await;
    Resp::json(200, &org)
}

/// `GET /admin/orgs/{id}` — one org by id, 404 if absent.
async fn orgs_get(state: &AppState, parts: &Parts, id: &str) -> Result<Resp, ApiError> {
    guard_admin(state, parts).await?;
    let id = parse_i64(id)?;
    let org = state
        .persistence
        .get_org(id)
        .await
        .map_err(internal)?
        .ok_or_else(|| ApiError::NotFound("not found".into()))?;
    Resp::json(200, &org)
}

/// `DELETE /admin/orgs/{id}` — 204 on removal, 404 otherwise; invalidate on hit.
async fn orgs_delete(state: &AppState, parts: &Parts, id: &str) -> Result<Resp, ApiError> {
    guard_admin(state, parts).await?;
    let id = parse_i64(id)?;
    if state.persistence.delete_org(id).await.map_err(internal)? {
        invalidate(state).await;
        Ok(Resp::no_content())
    } else {
        Err(ApiError::NotFound("not found".into()))
    }
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
