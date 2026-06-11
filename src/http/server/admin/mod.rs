//! Admin HTTP surface (native-only — uses axum). The `/admin/*` subrouter:
//! login/logout are public; everything else sits behind an admin session.

pub mod auth;
pub mod crud;
pub mod login;
pub mod middleware;
pub mod usage;

use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};

use crate::app::AppState;

/// Build the `/admin/*` subrouter. Returns a `Router<AppState>` (state is
/// applied by the caller's `.with_state`); `state` is threaded into the
/// middleware layer via [`from_fn_with_state`].
pub fn admin_router(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/admin/me", get(auth::me))
        // M10c — OAuth authcode login flow (start/complete), behind require_admin.
        .route("/admin/login-flows/start", post(login::start))
        .route("/admin/login-flows/complete", post(login::complete))
        // M10c — device-code login (copilot) + cookie login (claudecode).
        .route("/admin/login-flows/device/start", post(login::device_start))
        .route("/admin/login-flows/device/poll", post(login::device_poll))
        .route("/admin/login-flows/cookie", post(login::cookie))
        // M10b CRUD routes for the global config entities, all behind require_admin.
        .merge(crud::routes())
        // M10d read-only observability: usage, rollups, credential health, logs.
        .route("/admin/usage", get(usage::list_usage))
        .route("/admin/usage-rollups", get(usage::list_usage_rollups))
        .route(
            "/admin/credentials/{id}/status",
            get(usage::credential_status),
        )
        .route(
            "/admin/logs/{request_id}/downstream",
            get(usage::downstream_logs),
        )
        .route(
            "/admin/logs/{request_id}/upstream",
            get(usage::upstream_logs),
        )
        .layer(from_fn_with_state(state, middleware::require_admin));
    Router::new()
        .route("/admin/login", post(auth::login))
        .route("/admin/logout", post(auth::logout))
        .merge(protected)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    use crate::app::AppState;
    use crate::app::snapshot::ControlPlaneSnapshot;
    use crate::config::{CacheConfig, PersistenceConfig, RuntimeConfig, UpstreamConfig};
    use crate::http::client::{ClientError, RespStream, UpstreamClient};
    use crate::store::persistence::FilePersistence;
    use crate::store::persistence::records::{OrgInput, UserInput};

    /// admin tests never reach the upstream — a panicking stub suffices.
    struct NoUpstream;
    #[async_trait::async_trait]
    impl UpstreamClient for NoUpstream {
        async fn send(
            &self,
            _req: http::Request<bytes::Bytes>,
        ) -> Result<http::Response<bytes::Bytes>, ClientError> {
            unreachable!("admin tests do not call upstream")
        }
        async fn send_streaming(
            &self,
            _req: http::Request<bytes::Bytes>,
        ) -> Result<(StatusCode, http::HeaderMap, RespStream), ClientError> {
            unreachable!("admin tests do not call upstream")
        }
    }

    /// Build an AppState backed by a tempdir file store seeded with one admin
    /// user (`admin` / `secret`).
    async fn seeded_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let persistence: Arc<dyn crate::store::persistence::PersistenceBackend> = Arc::new(
            FilePersistence::open(dir.path().to_path_buf())
                .await
                .expect("open"),
        );
        let org = persistence
            .upsert_org(OrgInput {
                id: None,
                name: "default".into(),
                enabled: true,
                description: None,
            })
            .await
            .unwrap();
        persistence
            .upsert_user(UserInput {
                id: None,
                name: "admin".into(),
                org_id: org.id,
                team_id: None,
                password: Some(crate::crypto::password::hash("secret").unwrap()),
                enabled: true,
                is_admin: true,
            })
            .await
            .unwrap();

        let snapshot = ControlPlaneSnapshot::build(persistence.as_ref(), 1)
            .await
            .expect("snapshot");
        let config = Arc::new(RuntimeConfig {
            host: "127.0.0.1".into(),
            port: 0,
            cache: CacheConfig::Memory,
            persistence: PersistenceConfig::File {
                data_dir: dir.path().to_path_buf(),
            },
            upstream: UpstreamConfig::from_proxy_url(None),
            instance_id: 0,
            max_attempts: crate::config::DEFAULT_MAX_ATTEMPTS,
        });
        let cache: Arc<dyn crate::store::cache::CacheBackend> =
            Arc::new(crate::store::cache::MemoryCache::new());
        let snapshot = Arc::new(arc_swap::ArcSwap::from_pointee(snapshot));
        let channels = Arc::new(crate::channel::registry::ChannelRegistry::with_builtin());
        let state = AppState::new(
            config,
            cache,
            persistence,
            Arc::new(NoUpstream),
            snapshot,
            channels,
            Arc::new(crate::crypto::NoopCipher),
        );
        (state, dir)
    }

    /// `GPROXY_INSECURE_COOKIES=1` keeps the test cookie free of `Secure` so it
    /// round-trips over the in-process `oneshot` (no TLS).
    fn insecure_cookies() {
        // SAFETY: single-threaded test setup before any server call.
        unsafe { std::env::set_var("GPROXY_INSECURE_COOKIES", "1") };
    }

    #[tokio::test]
    async fn login_then_me_flow() {
        insecure_cookies();
        let (state, _dir) = seeded_state().await;
        let app = crate::http::server::router(state);

        // POST /admin/login → 200 + Set-Cookie.
        let resp = app
            .clone()
            .oneshot(
                Request::post("/admin/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"username":"admin","password":"secret"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .expect("Set-Cookie present")
            .to_string();
        assert!(set_cookie.contains("gproxy_session="), "{set_cookie}");
        let cookie = set_cookie.split(';').next().unwrap().to_string();

        // GET /admin/me with the cookie → 200 + the admin identity.
        let resp = app
            .oneshot(
                Request::get("/admin/me")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let me: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(me["name"], "admin");
        assert_eq!(me["is_admin"], true);
    }

    #[tokio::test]
    async fn login_bad_password_and_me_without_cookie_401() {
        insecure_cookies();
        let (state, _dir) = seeded_state().await;
        let app = crate::http::server::router(state);

        // Wrong password → 401.
        let resp = app
            .clone()
            .oneshot(
                Request::post("/admin/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"username":"admin","password":"wrong"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // No cookie on a protected route → 401.
        let resp = app
            .oneshot(Request::get("/admin/me").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
