//! Admin control-plane: session store + config CRUD invalidation helper.

pub mod login;
pub mod session;

/// After a config mutation: tell peers to reload (publish) and reload locally
/// now (so this instance serves the change immediately). The write is already
/// durable in persistence; a reload failure is logged, not surfaced.
#[cfg(not(target_arch = "wasm32"))]
pub async fn invalidate(state: &crate::app::AppState) {
    state
        .cache
        .publish(crate::store::cache::INVALIDATE_CHANNEL, b"config")
        .await;
    if let Err(e) = state.reload_snapshot().await {
        tracing::warn!(error = %e, "snapshot reload after admin mutation failed");
    }
}
