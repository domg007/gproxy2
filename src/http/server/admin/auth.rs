//! Admin auth endpoints: login (issue session cookie), logout (revoke), me.

use axum::extract::State;
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};

use crate::admin::session::{self, AdminUser};
use crate::api::auth::{LoginRequest, LoginResponse, MeResponse};
use crate::api::error::ApiError;
use crate::app::AppState;

/// `POST /admin/login`. Verifies credentials and issues a session cookie.
///
/// Every failure path returns a generic 401 — no user enumeration, no
/// distinction between "no such user", "not an admin", "disabled", or "wrong
/// password".
pub async fn login(State(state): State<AppState>, Json(req): Json<LoginRequest>) -> Response {
    let fail = || ApiError::Unauthorized.into_response();
    let Ok(Some(user)) = state.persistence.get_user_by_name(&req.username).await else {
        return fail();
    };
    if !user.enabled || !user.is_admin {
        return fail();
    }
    let Some(hash) = user.password.as_deref() else {
        return fail();
    };
    if !crate::crypto::password::verify(&req.password, hash) {
        return fail();
    }
    let token = session::create(state.cache.as_ref(), user.id).await;
    let body = LoginResponse {
        user: MeResponse {
            id: user.id,
            name: user.name.clone(),
            is_admin: user.is_admin,
        },
    };
    let cookie = session::set_cookie(&token, session::cookies_secure());
    ([(SET_COOKIE, cookie)], Json(body)).into_response()
}

/// `POST /admin/logout`. Revokes the current session (if any) and clears the
/// cookie. Always 204 — idempotent.
pub async fn logout(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
    if let Some(tok) = headers
        .get(COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(session::parse_cookie)
    {
        session::revoke(state.cache.as_ref(), tok).await;
    }
    let cookie = session::clear_cookie(session::cookies_secure());
    ([(SET_COOKIE, cookie)], axum::http::StatusCode::NO_CONTENT).into_response()
}

/// `GET /admin/me`. Runs behind [`super::middleware::require_admin`], so the
/// [`AdminUser`] extension is always present and always an admin.
pub async fn me(Extension(admin): Extension<AdminUser>) -> Json<MeResponse> {
    Json(MeResponse {
        id: admin.id,
        name: admin.name,
        is_admin: true,
    })
}
