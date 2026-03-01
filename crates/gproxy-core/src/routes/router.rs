use std::sync::Arc;

use axum::Router;
use axum::routing::post;

use crate::AppState;

pub fn management_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/login", post(super::login::login))
        .nest("/admin", super::admin::router())
        .nest("/user", super::user::router())
        .merge(super::provider::router())
        .with_state(state)
}
