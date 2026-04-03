use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

use gproxy_server::AppState;

pub mod credentials;
pub mod models;
pub mod permissions;
pub mod providers;
pub mod rate_limits;
pub mod requests;
pub mod settings;
pub mod usages;
pub mod users;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        // Global settings
        .route("/global-settings", get(settings::get_global_settings))
        .route("/global-settings/upsert", post(settings::upsert_global_settings))
        // Providers
        .route("/providers/query", post(providers::query_providers))
        .route("/providers/upsert", post(providers::upsert_provider))
        .route("/providers/delete", post(providers::delete_provider))
        // Credentials
        .route("/credentials/query", post(credentials::query_credentials))
        .route("/credentials/upsert", post(credentials::upsert_credential))
        .route("/credentials/delete", post(credentials::delete_credential))
        .route("/credential-statuses/query", post(credentials::query_credential_statuses))
        // Models
        .route("/models/query", post(models::query_models))
        .route("/models/upsert", post(models::upsert_model))
        .route("/models/delete", post(models::delete_model))
        .route("/model-aliases/query", post(models::query_model_aliases))
        .route("/model-aliases/upsert", post(models::upsert_model_alias))
        .route("/model-aliases/delete", post(models::delete_model_alias))
        // Users
        .route("/users/query", post(users::query_users))
        .route("/users/upsert", post(users::upsert_user))
        .route("/users/delete", post(users::delete_user))
        .route("/user-keys/query", post(users::query_user_keys))
        .route("/user-keys/generate", post(users::generate_user_key))
        .route("/user-keys/delete", post(users::delete_user_key))
        // Permissions
        .route("/user-permissions/query", post(permissions::query_permissions))
        .route("/user-permissions/upsert", post(permissions::upsert_permission))
        .route("/user-permissions/delete", post(permissions::delete_permission))
        // Rate limits
        .route("/user-rate-limits/query", post(rate_limits::query_rate_limits))
        .route("/user-rate-limits/upsert", post(rate_limits::upsert_rate_limit))
        .route("/user-rate-limits/delete", post(rate_limits::delete_rate_limit))
        // Requests
        .route("/requests/upstream/query", post(requests::query_upstream_requests))
        .route("/requests/upstream/count", post(requests::count_upstream_requests))
        .route("/requests/upstream/delete", post(requests::delete_upstream_requests))
        .route("/requests/downstream/query", post(requests::query_downstream_requests))
        .route("/requests/downstream/count", post(requests::count_downstream_requests))
        .route("/requests/downstream/delete", post(requests::delete_downstream_requests))
        // Usages
        .route("/usages/query", post(usages::query_usages))
        .route("/usages/count", post(usages::count_usages))
}
