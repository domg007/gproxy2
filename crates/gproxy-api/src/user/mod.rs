use std::sync::Arc;
use axum::Router;
use axum::routing::post;
use gproxy_server::AppState;

pub mod keys;
pub mod usages;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/keys/query", post(keys::query_keys))
        .route("/usages/query", post(usages::query_usages))
        .route("/usages/count", post(usages::count_usages))
}
