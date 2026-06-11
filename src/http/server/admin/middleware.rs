//! Admin auth middleware: cookie → session → enabled+is_admin, else 401.

use axum::extract::{Request, State};
use axum::http::{StatusCode, header::COOKIE};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::admin::session;
use crate::app::AppState;

/// Gate a router behind a valid admin session. On success the resolved
/// [`session::AdminUser`] is inserted into request extensions for handlers.
pub async fn require_admin(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = req
        .headers()
        .get(COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(session::parse_cookie);
    let Some(token) = token else {
        return unauthorized();
    };
    match session::validate(state.cache.as_ref(), state.persistence.as_ref(), token).await {
        Some(admin) => {
            req.extensions_mut().insert(admin);
            next.run(req).await
        }
        None => unauthorized(),
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({"error":{"message":"unauthorized","type":"admin_auth"}})),
    )
        .into_response()
}
