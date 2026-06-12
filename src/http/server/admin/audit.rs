//! Admin audit middleware: records an append-only row for every mutating
//! (non-GET) admin request. Runs INNER to [`super::middleware::require_admin`]
//! so the [`AdminUser`] extension is already set.
//!
//! The persistence write is fire-and-forget (`tokio::spawn`) so it never delays
//! the response. Only method/path/actor/status/ip are recorded — never the body,
//! headers, or any secret.

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;

use crate::admin::session::AdminUser;
use crate::app::AppState;
use crate::store::persistence::records::AuditLogInput;

/// Audit a mutating admin request. GET requests are not mutations and are
/// skipped; everything else is recorded after the handler runs.
pub async fn audit(State(state): State<AppState>, req: Request, next: Next) -> Response {
    // Reads aren't mutations — only audit POST/PUT/PATCH/DELETE (and others).
    if req.method() == Method::GET {
        return next.run(req).await;
    }

    let action = req.method().as_str().to_owned();
    let target = req.uri().path().to_owned();
    let source_ip = client_ip(&req);
    let (actor_id, actor_name) = match req.extensions().get::<AdminUser>() {
        Some(u) => (Some(u.id), Some(u.name.clone())),
        None => (None, None),
    };

    let resp = next.run(req).await;
    let status = resp.status().as_u16() as i64;

    // Fire-and-forget: don't make the client wait on the audit write.
    let persistence = state.persistence.clone();
    tokio::spawn(async move {
        let input = AuditLogInput {
            actor_id,
            actor_name,
            action,
            target,
            status,
            source_ip,
        };
        if let Err(e) = persistence.append_audit_log(input).await {
            tracing::warn!("audit log write failed: {e}");
        }
    });

    resp
}

/// Client IP from a reverse-proxy header (`x-forwarded-for` first hop, else
/// `x-real-ip`). gproxy runs behind a proxy, so the socket peer is the proxy.
fn client_ip(req: &Request) -> Option<String> {
    let headers = req.headers();
    headers
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .or_else(|| headers.get("x-real-ip").and_then(|h| h.to_str().ok()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
