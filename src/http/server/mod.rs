//! HTTP surface. Domain routers (admin, console) get nested here in later phases.

use axum::Router;
use axum::routing::get;

use crate::app::AppState;

mod health;
pub mod metrics;

// The gateway request path is `?Send` on wasm (FetchClient / libSQL), which axum
// 0.8's `Handler` (requires `Send` futures) rejects. Native wires the gateway as
// axum handlers; the edge fetch entry (`http::edge`) calls the same pipeline
// directly via `extract::build_ctx` + `pipeline::execute`, bypassing the router.
// `extract` is pure (http types only), so it compiles on both targets.
pub mod extract;
#[cfg(not(target_arch = "wasm32"))]
mod gateway;

#[cfg(not(target_arch = "wasm32"))]
pub mod admin;

/// Build the top-level axum router.
///
/// On native the literal `/v1/...` aggregated route is registered before the
/// `/{provider}/v1/...` scoped route; the scoped handler additionally rejects
/// `provider == "v1"`, so `v1` is reserved as a non-provider segment.
pub fn router(state: AppState) -> Router {
    #[allow(unused_mut)]
    let mut router = Router::new()
        .route("/healthz", get(health::healthz))
        .route("/version", get(health::version));

    #[cfg(not(target_arch = "wasm32"))]
    {
        use axum::extract::DefaultBodyLimit;
        use axum::routing::any;
        // 16 MiB inbound body limit — large LLM payloads, still bounded.
        router = router
            .route("/v1/{*rest}", any(gateway::aggregated))
            .route("/{provider}/v1/{*rest}", any(gateway::scoped))
            .layer(DefaultBodyLimit::max(16 * 1024 * 1024));
        router = router.route("/metrics", get(metrics::metrics));
        router = router.merge(admin::admin_router(state.clone()));
    }

    router.with_state(state)
}
