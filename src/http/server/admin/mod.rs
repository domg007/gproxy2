//! Admin HTTP surface (native-only — uses axum). The `/admin/*` subrouter:
//! login/logout are public; everything else sits behind admin auth (session
//! cookie or an admin user's API key — see `crate::admin::authenticate_admin`).

pub mod audit;
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
        // M10d audit log: most-recent mutating-admin-action trail.
        .route("/admin/audit", get(usage::list_audit))
        // Audit middleware runs INNER to require_admin (added first = innermost),
        // so the AdminUser extension is set when it records a mutating request.
        .layer(from_fn_with_state(state.clone(), audit::audit))
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
    use crate::store::persistence::records::{OrgInput, UserInput, UserKeyInput};

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
            max_in_flight: crate::config::DEFAULT_MAX_IN_FLIGHT,
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

    #[tokio::test]
    async fn login_brute_force_locks_out_after_threshold() {
        insecure_cookies();
        let (state, _dir) = seeded_state().await;
        let app = crate::http::server::router(state);
        let bad = || {
            Request::post("/admin/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"username":"admin","password":"wrong"}"#))
                .unwrap()
        };
        // 5 wrong passwords → 401 each; the 6th is locked out with 429.
        for _ in 0..5 {
            let resp = app.clone().oneshot(bad()).await.unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }
        let resp = app.clone().oneshot(bad()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        // Even the CORRECT password is throttled while locked out.
        let good = app
            .oneshot(
                Request::post("/admin/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"username":"admin","password":"secret"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(good.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn mutating_request_and_failed_login_are_audited() {
        insecure_cookies();
        let (state, _dir) = seeded_state().await;
        let persistence = state.persistence.clone();
        let app = crate::http::server::router(state);

        // A failed login records a `login.fail` row (no cookie needed; public).
        let resp = app
            .clone()
            .oneshot(
                Request::post("/admin/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"username":"ghost","password":"nope"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Log in to get a session cookie for the mutating request.
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
        let cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();

        // A mutating (DELETE) admin request flows through the audit middleware.
        let resp = app
            .clone()
            .oneshot(
                Request::delete("/admin/orgs/99999")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Whatever the outcome, the request was authenticated and audited.
        assert!(resp.status().is_success() || resp.status().is_client_error());

        // Audit writes are fire-and-forget; give the spawned tasks a moment.
        for _ in 0..50 {
            tokio::task::yield_now().await;
            if persistence.list_audit_logs(100).await.unwrap().len() >= 3 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let rows = persistence.list_audit_logs(100).await.unwrap();

        // The DELETE was recorded with method=action and path=target.
        assert!(
            rows.iter()
                .any(|r| r.action == "DELETE" && r.target == "/admin/orgs/99999"),
            "expected DELETE audit row, got {rows:?}"
        );
        // The failed login was recorded; never the password.
        assert!(
            rows.iter()
                .any(|r| r.action == "login.fail" && r.target == "ghost" && r.actor_id.is_none()),
            "expected login.fail audit row, got {rows:?}"
        );
        // The successful login was recorded too.
        assert!(
            rows.iter()
                .any(|r| r.action == "login.success" && r.actor_name.as_deref() == Some("admin")),
            "expected login.success audit row, got {rows:?}"
        );
    }

    /// Headless admin auth: an enabled admin user's API key passes
    /// `require_admin` via either header form; non-admin / unknown keys don't.
    #[tokio::test]
    async fn admin_api_key_auth() {
        let (state, _dir) = seeded_state().await;
        let admin = state
            .persistence
            .get_user_by_name("admin")
            .await
            .unwrap()
            .unwrap();
        let plain = state
            .persistence
            .upsert_user(UserInput {
                id: None,
                name: "plain".into(),
                org_id: admin.org_id,
                team_id: None,
                password: None,
                enabled: true,
                is_admin: false,
            })
            .await
            .unwrap();
        for (user_id, tok) in [(admin.id, "admin-tok"), (plain.id, "plain-tok")] {
            state
                .persistence
                .upsert_user_key(UserKeyInput {
                    id: None,
                    user_id,
                    api_key_ciphertext: String::new(),
                    api_key_digest: crate::pipeline::auth::key_digest(tok),
                    label: None,
                    enabled: true,
                })
                .await
                .unwrap();
        }
        state.reload_snapshot().await.unwrap();
        let app = crate::http::server::router(state);

        for (name, value, expect) in [
            ("x-api-key", "admin-tok", StatusCode::OK),
            ("authorization", "Bearer admin-tok", StatusCode::OK),
            ("x-api-key", "plain-tok", StatusCode::UNAUTHORIZED),
            ("x-api-key", "no-such-key", StatusCode::UNAUTHORIZED),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::get("/admin/me")
                        .header(name, value)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), expect, "{name}: {value}");
        }
    }
}
