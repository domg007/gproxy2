//! HTTP surface. Domain routers (admin, console) get nested here in later phases.

use axum::Router;
use axum::routing::get;

use crate::app::AppState;

mod health;

// The gateway request path is `?Send` on wasm (FetchClient / libSQL), which axum
// 0.8's `Handler` (requires `Send` futures) rejects. M1 wires the gateway on
// native only; edge serves health until the edge phase resolves the Send seam.
#[cfg(not(target_arch = "wasm32"))]
mod extract;
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
    }

    router.with_state(state)
}
