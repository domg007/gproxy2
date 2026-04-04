use std::sync::Arc;

use axum::Router;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{get, post};

use gproxy_server::AppState;
use gproxy_server::middleware::classify::classify_middleware;
use gproxy_server::middleware::model_alias::model_alias_middleware;
use gproxy_server::middleware::request_model::request_model_middleware;

use gproxy_server::middleware::sanitize::sanitize_middleware;

pub mod handler;
pub mod oauth;
pub mod websocket;
pub mod ws_bridge;

pub fn router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        // Scoped routes: /{provider}/v1/...
        .route("/{provider}/v1/messages", post(handler::proxy))
        .route("/{provider}/v1/messages/count-tokens", post(handler::proxy))
        .route("/{provider}/v1/chat/completions", post(handler::proxy))
        .route(
            "/{provider}/v1/responses",
            post(handler::proxy).get(websocket::openai_responses_ws),
        )
        .route(
            "/{provider}/v1/responses/input_tokens",
            post(handler::proxy),
        )
        .route("/{provider}/v1/responses/compact", post(handler::proxy))
        .route("/{provider}/v1/embeddings", post(handler::proxy))
        .route("/{provider}/v1/images/generations", post(handler::proxy))
        .route("/{provider}/v1/images/edits", post(handler::proxy))
        .route(
            "/{provider}/v1/files",
            post(handler::proxy).get(handler::proxy),
        )
        .route(
            "/{provider}/v1/files/{file_id}",
            get(handler::proxy).delete(handler::proxy),
        )
        .route(
            "/{provider}/v1/files/{file_id}/content",
            get(handler::proxy),
        )
        .route("/{provider}/v1/models", get(handler::proxy))
        .route("/{provider}/v1/models/{*model_id}", get(handler::proxy))
        .route("/{provider}/v1beta/models", get(handler::proxy))
        .route("/{provider}/v1beta/{*target}", post(handler::proxy))
        // Gemini Live WebSocket
        .route(
            "/{provider}/v1beta/models/{*target_live}",
            get(websocket::gemini_live),
        )
        // OAuth
        .route("/{provider}/v1/oauth", get(oauth::oauth_start))
        .route("/{provider}/v1/oauth/callback", get(oauth::oauth_callback))
        .route("/{provider}/v1/usage", get(oauth::upstream_usage))
        // Unscoped routes (provider determined by model prefix or alias)
        .route("/v1/messages", post(handler::proxy_unscoped))
        .route("/v1/messages/count_tokens", post(handler::proxy_unscoped))
        .route("/v1/chat/completions", post(handler::proxy_unscoped))
        .route(
            "/v1/responses",
            post(handler::proxy_unscoped).get(websocket::openai_responses_ws_unscoped),
        )
        .route("/v1/responses/input_tokens", post(handler::proxy_unscoped))
        .route("/v1/responses/compact", post(handler::proxy_unscoped))
        .route("/v1/embeddings", post(handler::proxy_unscoped))
        .route("/v1/images/generations", post(handler::proxy_unscoped))
        .route("/v1/images/edits", post(handler::proxy_unscoped))
        .route("/v1/models", get(handler::proxy_unscoped))
        .route("/v1/models/{*model_id}", get(handler::proxy_unscoped))
        // Unscoped file operations (provider from X-Provider header)
        .route(
            "/v1/files",
            post(handler::proxy_unscoped_files).get(handler::proxy_unscoped_files),
        )
        .route(
            "/v1/files/{file_id}",
            get(handler::proxy_unscoped_files).delete(handler::proxy_unscoped_files),
        )
        .route(
            "/v1/files/{file_id}/content",
            get(handler::proxy_unscoped_files),
        )
        // Unscoped Gemini v1beta routes (model in path carries provider prefix)
        .route("/v1beta/models", get(handler::proxy_unscoped))
        .route("/v1beta/{*target}", post(handler::proxy_unscoped))
        // Middleware: classify → request_model → model_alias → sanitize → handler
        .layer(from_fn(sanitize_middleware))
        .layer(from_fn_with_state(state, model_alias_middleware))
        .layer(from_fn(request_model_middleware))
        .layer(from_fn(classify_middleware))
}
