//! OAuth login-flow endpoints (§14.5, native-only — uses axum). Drives the
//! interactive authcode dance for credential channels behind `require_admin`.
//!
//! `start` builds the authorize URL (PKCE + CSRF state stashed in the cache);
//! `complete` parses the provider callback, verifies the state, exchanges the
//! code for a secret, seals + persists it as a credential, and returns the
//! redacted [`CredentialView`]. The auth code, PKCE verifier, and plaintext
//! secret are NEVER logged; every failure collapses to a generic 4xx.

use axum::Json;
use axum::extract::State;

use crate::admin::{invalidate, login};
use crate::api::credentials::CredentialView;
use crate::api::error::ApiError;
use crate::api::login::{LoginCompleteRequest, LoginStartRequest, LoginStartResponse};
use crate::app::AppState;
use crate::channel::oauth;
use crate::store::persistence::records::CredentialInput;
use crate::util::rand::uuid_v4;

/// `POST /admin/login-flows/start`. Resolves the channel's authcode login,
/// mints PKCE + CSRF state, stashes them in the cache, and returns the
/// authorize URL the admin sends the user to.
pub async fn start(
    State(state): State<AppState>,
    Json(req): Json<LoginStartRequest>,
) -> Result<Json<LoginStartResponse>, ApiError> {
    let channel = state
        .channels
        .login_for(&req.channel)
        .ok_or_else(|| ApiError::NotFound("unknown channel".into()))?;

    let (verifier, challenge) = oauth::pkce();
    let state_tok = uuid_v4();
    let started = channel
        .authcode_start(
            req.redirect_uri.as_deref().unwrap_or_default(),
            &state_tok,
            &challenge,
        )
        .ok_or_else(|| ApiError::BadRequest("channel has no authcode login".into()))?;

    let sid = login::start(
        state.cache.as_ref(),
        req.channel,
        verifier,
        state_tok,
        started.redirect_uri,
    )
    .await;

    Ok(Json(LoginStartResponse {
        login_session_id: sid,
        authorize_url: started.authorize_url,
    }))
}

/// `POST /admin/login-flows/complete`. Consumes the pending login, verifies the
/// CSRF state, exchanges the callback code for a secret, and persists it as a
/// sealed credential under `provider_id`.
pub async fn complete(
    State(state): State<AppState>,
    Json(req): Json<LoginCompleteRequest>,
) -> Result<Json<CredentialView>, ApiError> {
    let bad = || ApiError::BadRequest("login failed".into());

    let (code, cb_state) = parse_callback(&req.callback_url).ok_or_else(bad)?;
    let session = login::take(state.cache.as_ref(), &req.login_session_id)
        .await
        .ok_or_else(bad)?;
    // CSRF: the callback state MUST match the one we issued.
    if cb_state != session.state {
        return Err(bad());
    }

    let channel = state.channels.login_for(&session.channel).ok_or_else(bad)?;
    let secret = channel
        .authcode_exchange(
            &state.upstream,
            &code,
            &session.verifier,
            &session.redirect_uri,
        )
        .await
        .map_err(|_| bad())?;

    let sealed = state.cipher.seal(&secret).map_err(|_| bad())?;
    let input = CredentialInput {
        id: None,
        provider_id: req.provider_id,
        name: req.name,
        kind: "oauth".into(),
        secret_json: sealed,
        weight: 100,
        rpm_limit: None,
        tpm_limit: None,
        proxy_url: None,
        tls_fingerprint: None,
        enabled: true,
    };
    let cred = state
        .persistence
        .upsert_credential(input)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    invalidate(&state).await;
    Ok(Json(CredentialView::from(cred)))
}

/// Pull `code` + `state` out of a callback URL's query string. No `url` dep:
/// `http::Uri` splits off the query, then a manual `&`/`=` walk with
/// percent-decoding. Both params are required.
fn parse_callback(callback_url: &str) -> Option<(String, String)> {
    let uri: http::Uri = callback_url.parse().ok()?;
    let query = uri.query()?;
    let mut code = None;
    let mut state = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        match k {
            "code" => code = Some(pct_decode(v)),
            "state" => state = Some(pct_decode(v)),
            _ => {}
        }
    }
    Some((code?, state?))
}

/// Percent-decode a query value (`+` → space, `%XX` → byte). Lossy on invalid
/// UTF-8; malformed `%` escapes are kept verbatim.
fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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
    use crate::store::persistence::records::{OrgInput, ProviderInput, UserInput};

    /// Upstream stub: any token-endpoint POST returns a canned token response.
    struct FakeUpstream;
    #[async_trait::async_trait]
    impl UpstreamClient for FakeUpstream {
        async fn send(
            &self,
            _req: http::Request<bytes::Bytes>,
        ) -> Result<http::Response<bytes::Bytes>, ClientError> {
            let body = br#"{"access_token":"at-1","refresh_token":"rt-1","expires_in":3600}"#;
            Ok(http::Response::builder()
                .status(200)
                .body(bytes::Bytes::from_static(body))
                .unwrap())
        }
        async fn send_streaming(
            &self,
            _req: http::Request<bytes::Bytes>,
        ) -> Result<(StatusCode, http::HeaderMap, RespStream), ClientError> {
            unreachable!("login tests do not stream")
        }
    }

    async fn state_and_provider() -> (AppState, tempfile::TempDir, i64, i64) {
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
        let admin = persistence
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
        let provider = persistence
            .upsert_provider(ProviderInput {
                id: None,
                name: "codex".into(),
                channel: "codex".into(),
                label: None,
                settings_json: serde_json::json!({}),
                credential_strategy: "weighted".into(),
                proxy_url: None,
                tls_fingerprint: None,
                enabled: true,
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
            Arc::new(FakeUpstream),
            snapshot,
            channels,
            Arc::new(crate::crypto::NoopCipher),
        );
        (state, dir, admin.id, provider.id)
    }

    async fn admin_cookie(state: &AppState, admin_id: i64) -> String {
        let token = crate::admin::session::create(state.cache.as_ref(), admin_id).await;
        format!("{}={token}", crate::admin::session::cookie_name())
    }

    /// Extract the `state=` query param value from an authorize URL.
    fn state_from_url(url: &str) -> String {
        let q = url.split_once('?').unwrap().1;
        q.split('&')
            .find_map(|p| p.strip_prefix("state="))
            .unwrap()
            .to_string()
    }

    /// start → authorize URL; complete with the matching state → 200 + a sealed
    /// credential; a mismatched state → 400.
    #[tokio::test]
    async fn login_start_complete_flow() {
        // SAFETY: single-threaded test setup before any server call.
        unsafe { std::env::set_var("GPROXY_INSECURE_COOKIES", "1") };
        let (state, _dir, admin_id, provider_id) = state_and_provider().await;
        let cookie = admin_cookie(&state, admin_id).await;
        let persistence = state.persistence.clone();
        let app = crate::http::server::router(state);

        // start
        let resp = app
            .clone()
            .oneshot(
                Request::post("/admin/login-flows/start")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"channel":"codex"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let started: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let sid = started["login_session_id"].as_str().unwrap().to_string();

        // complete with a mismatched state → 400 (this also consumes the session).
        let resp = app
            .clone()
            .oneshot(
                Request::post("/admin/login-flows/complete")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"login_session_id":"{sid}","callback_url":"http://x/cb?code=abc&state=WRONG","provider_id":{provider_id}}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Re-run start to get a fresh one-shot session, then complete with the
        // correct state → 200 + a credential.
        let resp = app
            .clone()
            .oneshot(
                Request::post("/admin/login-flows/start")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"channel":"codex"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let started: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let sid = started["login_session_id"].as_str().unwrap().to_string();
        let csrf = state_from_url(started["authorize_url"].as_str().unwrap());
        assert_ne!(csrf, "WRONG");

        let resp = app
            .oneshot(
                Request::post("/admin/login-flows/complete")
                    .header(header::COOKIE, &cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{"login_session_id":"{sid}","callback_url":"http://x/cb?code=abc&state={csrf}","provider_id":{provider_id}}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let view: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(view.get("secret_json").is_none(), "must not leak secret");
        assert_eq!(view["has_secret"], true);
        assert_eq!(view["kind"], "oauth");

        // The persisted secret carries the exchanged tokens (NoopCipher = plain).
        let cred_id = view["id"].as_i64().unwrap();
        let stored = persistence.get_credential(cred_id).await.unwrap().unwrap();
        assert_eq!(stored.secret_json["access_token"], "at-1");
        assert_eq!(stored.secret_json["refresh_token"], "rt-1");
    }
}
