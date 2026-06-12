//! Admin auth middleware: session cookie OR an admin user's API key, else 401.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::api::error::ApiError;
use crate::app::AppState;

/// Gate a router behind [`authenticate_admin`](crate::admin::authenticate_admin)
/// (admin session cookie or an admin user's API key). On success the resolved
/// [`AdminUser`](crate::admin::session::AdminUser) is inserted into request
/// extensions for handlers.
pub async fn require_admin(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    match crate::admin::authenticate_admin(&state, req.headers()).await {
        Some(admin) => {
            req.extensions_mut().insert(admin);
            next.run(req).await
        }
        None => ApiError::Unauthorized.into_response(),
    }
}
