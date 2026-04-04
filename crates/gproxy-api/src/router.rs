use std::sync::Arc;

use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::routing::post;

use gproxy_server::AppState;

use crate::auth::{require_admin_middleware, require_user_middleware};
use crate::cors::CorsLayer;

/// Build the complete API router.
pub fn api_router(state: Arc<AppState>) -> Router {
    let admin_router =
        crate::admin::router().layer(from_fn_with_state(state.clone(), require_admin_middleware));
    let user_router =
        crate::user::router().layer(from_fn_with_state(state.clone(), require_user_middleware));

    Router::new()
        .route("/login", post(crate::login::login))
        .nest("/admin", admin_router)
        .nest("/user", user_router)
        .merge(crate::provider::router(state.clone()))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
