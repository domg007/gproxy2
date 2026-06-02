//! HTTP surface. Domain routers (gateway, admin, console) get nested
//! here in later phases.

use axum::Router;
use axum::routing::get;

use crate::app::AppState;

mod health;

/// Build the top-level axum router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/version", get(health::version))
        .with_state(state)
}
