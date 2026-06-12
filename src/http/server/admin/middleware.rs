//! Admin auth middleware: session cookie OR an admin user's API key, else 401.
//! State-changing cookie-authenticated requests also pass a same-origin (CSRF)
//! check before auth.

use axum::extract::{Request, State};
use axum::http::Method;
use axum::http::header::{COOKIE, HOST, ORIGIN, REFERER};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::api::error::ApiError;
use crate::app::AppState;

/// Gate a router behind [`authenticate_admin`](crate::admin::authenticate_admin)
/// (admin session cookie or an admin user's API key). On success the resolved
/// [`AdminUser`](crate::admin::session::AdminUser) is inserted into request
/// extensions for handlers.
///
/// A same-origin (CSRF) check runs first for state-changing methods — see
/// [`csrf_ok`]. It only constrains cookie-authenticated browser requests;
/// header (API-key) automation is untouched.
pub async fn require_admin(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    if !csrf_ok(&req) {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "cross-origin admin request refused",
        )
            .into_response();
    }
    match crate::admin::authenticate_admin(&state, req.headers()).await {
        Some(admin) => {
            req.extensions_mut().insert(admin);
            next.run(req).await
        }
        None => ApiError::Unauthorized.into_response(),
    }
}

/// Same-origin guard for cookie-authenticated mutations — defense-in-depth atop
/// the session cookie's `SameSite=Lax` attribute.
///
/// Enforced ONLY when (a) the method is state-changing and (b) the request
/// carries a session cookie. A browser auto-sends the cookie cross-site but
/// cannot forge/suppress an `Origin` on a non-GET fetch; header-auth (API key)
/// clients send no session cookie, so curl/CI automation is never affected.
///
/// Decision: if neither `Origin` nor `Referer` is present we pass (no browser
/// cross-origin signal exists — `SameSite=Lax` is the backstop, and non-browser
/// callers don't carry the ambient cookie anyway). If either header IS present,
/// its authority must equal the request's own `Host`; a cross-origin or
/// unparseable (`Origin: null`) value is refused. NOTE: behind a reverse proxy
/// the `Host` header must reflect the public origin (`proxy_set_header Host
/// $host`).
fn csrf_ok(req: &Request) -> bool {
    let state_changing = !matches!(
        *req.method(),
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    );
    if !state_changing {
        return true;
    }
    let has_session = req
        .headers()
        .get(COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(crate::admin::session::parse_cookie)
        .is_some();
    if !has_session {
        return true;
    }
    let origin = req.headers().get(ORIGIN);
    let referer = req.headers().get(REFERER);
    if origin.is_none() && referer.is_none() {
        // No cross-origin signal to verify; SameSite=Lax already blocks the
        // cross-site form/navigation case for a non-forgeable cookie.
        return true;
    }
    let host = req
        .headers()
        .get(HOST)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_ascii_lowercase())
        .or_else(|| {
            req.uri()
                .authority()
                .map(|a| a.as_str().to_ascii_lowercase())
        });
    let claimed = origin
        .and_then(|h| h.to_str().ok())
        .and_then(authority_of)
        .or_else(|| referer.and_then(|h| h.to_str().ok()).and_then(authority_of));
    matches!((host, claimed), (Some(h), Some(c)) if h == c)
}

/// Extract the lowercased `host[:port]` authority from an absolute URL
/// (`scheme://host[:port]/...`). `Origin: null` and relative values yield
/// `None`.
fn authority_of(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1)?;
    let authority = after_scheme.split('/').next()?.trim();
    (!authority.is_empty()).then(|| authority.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn req(
        method: Method,
        cookie: Option<&str>,
        host: Option<&str>,
        origin: Option<&str>,
    ) -> Request {
        let mut b = Request::builder().method(method).uri("/admin/orgs/1");
        if let Some(c) = cookie {
            b = b.header(COOKIE, c);
        }
        if let Some(h) = host {
            b = b.header(HOST, h);
        }
        if let Some(o) = origin {
            b = b.header(ORIGIN, o);
        }
        b.body(Body::empty()).unwrap()
    }

    const SESSION: &str = "gproxy_session=abc123";

    #[test]
    fn get_is_never_blocked() {
        assert!(csrf_ok(&req(
            Method::GET,
            Some(SESSION),
            Some("h"),
            Some("https://evil")
        )));
    }

    #[test]
    fn header_auth_mutation_without_session_cookie_passes() {
        // No session cookie → API-key/automation path, CSRF N/A.
        assert!(csrf_ok(&req(
            Method::DELETE,
            None,
            Some("gp.example"),
            None
        )));
        // An unrelated cookie that isn't our session also passes.
        assert!(csrf_ok(&req(
            Method::POST,
            Some("other=1"),
            Some("gp.example"),
            Some("https://evil.example")
        )));
    }

    #[test]
    fn same_origin_cookie_mutation_passes() {
        assert!(csrf_ok(&req(
            Method::POST,
            Some(SESSION),
            Some("gp.example"),
            Some("https://gp.example")
        )));
        // Port-qualified origins match the Host when both carry the port.
        assert!(csrf_ok(&req(
            Method::DELETE,
            Some(SESSION),
            Some("gp.example:8443"),
            Some("https://gp.example:8443/console")
        )));
    }

    #[test]
    fn cross_origin_cookie_mutation_refused() {
        assert!(!csrf_ok(&req(
            Method::POST,
            Some(SESSION),
            Some("gp.example"),
            Some("https://evil.example")
        )));
    }

    #[test]
    fn cookie_mutation_without_origin_or_referer_passes() {
        // No browser cross-origin signal at all → SameSite=Lax backstop applies.
        assert!(csrf_ok(&req(
            Method::DELETE,
            Some(SESSION),
            Some("gp.example"),
            None
        )));
    }

    #[test]
    fn cross_origin_referer_refused_when_origin_absent() {
        let r = Request::builder()
            .method(Method::POST)
            .uri("/admin/orgs/1")
            .header(COOKIE, SESSION)
            .header(HOST, "gp.example")
            .header(REFERER, "https://evil.example/x")
            .body(Body::empty())
            .unwrap();
        assert!(!csrf_ok(&r));
    }

    #[test]
    fn origin_null_refused() {
        assert!(!csrf_ok(&req(
            Method::POST,
            Some(SESSION),
            Some("gp.example"),
            Some("null")
        )));
    }
}
