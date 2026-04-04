use std::sync::Arc;

use axum::Router;
use axum::routing::post;

use gproxy_server::AppState;

use crate::cors::CorsLayer;

/// Build the complete API router.
pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/login", post(crate::login::login))
        .nest("/admin", crate::admin::router())
        .nest("/user", crate::user::router())
        .merge(crate::provider::router(state.clone()))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
