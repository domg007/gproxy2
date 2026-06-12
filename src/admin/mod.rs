//! Admin control-plane: caller auth + session store + config CRUD
//! invalidation helper.

pub mod login;
pub mod session;

use http::HeaderMap;
use http::header::COOKIE;

use crate::app::AppState;

/// Resolve the admin caller behind a request — THE auth check for every
/// admin-gated surface (native `/admin/*` + `/healthz` + `/metrics` via
/// `require_admin`, and the edge fetch arms). Two accepted forms:
/// 1. session cookie (browser/console): revocable, re-checked against
///    persistence per request ([`session::validate`]);
/// 2. API key of an enabled admin user (headless: curl / Prometheus / CI),
///    header forms only — no `?key=` fallback, admin URLs end up in logs.
///    Resolved against the control-plane snapshot exactly like gateway auth
///    (native reloads it on every config mutation; an edge isolate keeps its
///    boot snapshot until recycled).
///
/// An expired/invalid cookie falls through to the key check, not straight 401.
pub async fn authenticate_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<session::AdminUser> {
    if let Some(token) = headers
        .get(COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(session::parse_cookie)
        && let Some(admin) =
            session::validate(state.cache.as_ref(), state.persistence.as_ref(), token).await
    {
        return Some(admin);
    }
    let cp = state.cp();
    let identity = crate::pipeline::auth::authenticate(&cp, headers, None).ok()?;
    identity.user.is_admin.then(|| session::AdminUser {
        id: identity.user.id,
        name: identity.user.name.clone(),
    })
}

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
