use std::sync::Arc;

use axum::Router;

use crate::AppState;

pub fn management_router(state: Arc<AppState>) -> Router {
    Router::new()
        .nest("/admin", super::admin::router())
        .nest("/user", super::user::router())
        .merge(super::provider::router())
        .with_state(state)
}
