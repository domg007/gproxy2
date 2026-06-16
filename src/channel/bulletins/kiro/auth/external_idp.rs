//! Kiro external-IdP login — a generic operator-configured OIDC provider.
//!
//! Standard OIDC `authorization_code` + PKCE (S256) against an arbitrary IdP, NO
//! dynamic registration, public client (no `client_secret`). Replicates kiro-cli
//! `fig_auth::external_idp`:
//!   1. discovery  `GET {issuer_url}/.well-known/openid-configuration` →
//!      `{authorization_endpoint, token_endpoint}`.
//!   2. authorize  `GET {authorization_endpoint}?…&scope=<configured + offline_access>`.
//!   3. token      `POST {token_endpoint}` form grant=`authorization_code`.
//!   4. refresh    `POST {token_endpoint}` form grant=`refresh_token`.
//!
//! Operator params (required): `issuer_url`, `client_id`. Optional: `scopes`
//! (space-joined; `offline_access` is always appended so the IdP mints a refresh
//! token). The token exchange / refresh reuse [`oauth::token_post`]
//! (form-urlencoded). `token_endpoint`/`client_id`/`issuer_url` are stashed in
//! the secret so [`refresh`] needs no re-discovery — and `token_endpoint`'s
//! presence is the refresh discriminator (see [`super::refresh`]).

use std::sync::Arc;

use bytes::Bytes;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::channel::ChannelError;
use crate::channel::login::AuthCodeStart;
use crate::channel::oauth;
use crate::http::client::UpstreamClient;

use super::{now_ms, secret_str};

/// Default loopback redirect when the console passes none (must match the
/// redirect registered at the operator's IdP; callback-URL mode passes its own).
const DEFAULT_REDIRECT_URI: &str = "http://127.0.0.1:1455/oauth/callback";
/// Always appended to the configured scopes so the IdP mints a refresh token
/// (RE: kiro-cli hardcodes this into the external_idp scope set).
const OFFLINE_ACCESS: &str = "offline_access";

/// `GET {issuer}/.well-known/openid-configuration` subset (the two endpoints we
/// need; the rest of the OIDC discovery document is ignored).
#[derive(Debug, Deserialize)]
struct OidcDiscovery {
    authorization_endpoint: Option<String>,
    token_endpoint: Option<String>,
}

/// Build the space-joined scope string: the operator's `params.scopes` (default
/// `openid`) with `offline_access` appended if absent.
fn scope_string(params: &Value) -> String {
    let configured = secret_str(params, "scopes").unwrap_or("openid");
    let mut scopes: Vec<&str> = configured.split_whitespace().collect();
    if !scopes.contains(&OFFLINE_ACCESS) {
        scopes.push(OFFLINE_ACCESS);
    }
    scopes.join(" ")
}

/// `GET {issuer_url}/.well-known/openid-configuration` → `(authorization_endpoint,
/// token_endpoint)`.
async fn fetch_discovery(
    client: &Arc<dyn UpstreamClient>,
    issuer_url: &str,
) -> Result<(String, String), ChannelError> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer_url.trim_end_matches('/')
    );
    let req = http::Request::get(&url)
        .header(http::header::ACCEPT, "application/json")
        .body(Bytes::new())
        .map_err(|e| ChannelError::Build(format!("oidc discovery request build: {e}")))?;
    let resp = client
        .send(req)
        .await
        .map_err(|e| ChannelError::Build(format!("oidc discovery request failed: {e}")))?;
    let (parts, body) = resp.into_parts();
    if !parts.status.is_success() {
        let snippet: String = String::from_utf8_lossy(&body).chars().take(256).collect();
        return Err(ChannelError::Build(format!(
            "oidc discovery {}: {snippet}",
            parts.status
        )));
    }
    let doc: OidcDiscovery = serde_json::from_slice(&body)
        .map_err(|e| ChannelError::Build(format!("oidc discovery parse: {e}")))?;
    let authorization_endpoint = doc
        .authorization_endpoint
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            ChannelError::Build("oidc discovery missing authorization_endpoint".into())
        })?;
    let token_endpoint = doc
        .token_endpoint
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("oidc discovery missing token_endpoint".into()))?;
    Ok((authorization_endpoint, token_endpoint))
}

/// Begin the external-IdP authcode+PKCE login: OIDC discovery, then build the
/// authorization URL. Stashes `{token_endpoint, client_id, issuer_url, scopes}`
/// in `extra` for [`authcode_exchange`].
pub(super) async fn authcode_start(
    client: &Arc<dyn UpstreamClient>,
    params: &Value,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> Result<AuthCodeStart, ChannelError> {
    let issuer_url = secret_str(params, "issuer_url").ok_or_else(|| {
        ChannelError::Build("external_idp login requires params.issuer_url".into())
    })?;
    let client_id = secret_str(params, "client_id").ok_or_else(|| {
        ChannelError::Build("external_idp login requires params.client_id".into())
    })?;
    let scopes = scope_string(params);
    let redirect_uri = if redirect_uri.trim().is_empty() {
        DEFAULT_REDIRECT_URI
    } else {
        redirect_uri
    };

    let (authorization_endpoint, token_endpoint) = fetch_discovery(client, issuer_url).await?;
    let authorize_url = authorize_url(
        &authorization_endpoint,
        client_id,
        redirect_uri,
        &scopes,
        state,
        challenge,
    );

    Ok(AuthCodeStart {
        authorize_url,
        redirect_uri: redirect_uri.to_string(),
        extra: Some(json!({
            "token_endpoint": token_endpoint,
            "client_id": client_id,
            "issuer_url": issuer_url,
            "scopes": scopes,
        })),
    })
}

/// Build a standard OIDC `/authorize` URL (singular `scope`, space-joined, S256).
fn authorize_url(
    authorization_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    scopes: &str,
    state: &str,
    challenge: &str,
) -> String {
    let query = [
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", scopes),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    let mut out = String::new();
    for (k, v) in query {
        out.push(if out.is_empty() { '?' } else { '&' });
        out.push_str(&oauth::percent_encode(k));
        out.push('=');
        out.push_str(&oauth::percent_encode(v));
    }
    // The endpoint may already carry a query (rare); join with the right sep.
    let sep = if authorization_endpoint.contains('?') {
        out.replacen('?', "&", 1)
    } else {
        out
    };
    format!("{authorization_endpoint}{sep}")
}

/// Exchange the authcode (+PKCE verifier) for tokens via `POST {token_endpoint}`
/// (form-urlencoded, grant=`authorization_code`, public client — no secret). The
/// minted secret carries `token_endpoint`/`client_id`/`issuer_url` for refresh.
pub(super) async fn authcode_exchange(
    client: &Arc<dyn UpstreamClient>,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    extra: &Value,
) -> Result<Value, ChannelError> {
    let token_endpoint = secret_str(extra, "token_endpoint")
        .ok_or_else(|| ChannelError::Build("login session missing token_endpoint".into()))?;
    let client_id = secret_str(extra, "client_id")
        .ok_or_else(|| ChannelError::Build("login session missing client_id".into()))?;
    let issuer_url = secret_str(extra, "issuer_url").unwrap_or_default();

    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", verifier),
        ("client_id", client_id),
    ];
    let resp = oauth::token_post(client, token_endpoint, &form, &[]).await?;

    let access_token = resp
        .access_token
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChannelError::Build("token response missing access_token".into()))?;
    let expires_at_ms = now_ms() + resp.expires_in.unwrap_or(3600) as i64 * 1000;

    let mut secret = json!({
        "access_token": access_token,
        "expires_at_ms": expires_at_ms,
        "token_endpoint": token_endpoint,
        "client_id": client_id,
        "issuer_url": issuer_url,
    });
    if let Some(rt) = resp.refresh_token.filter(|s| !s.is_empty()) {
        secret["refresh_token"] = Value::String(rt);
    }
    Ok(secret)
}

/// Refresh via `POST {token_endpoint}` form grant=`refresh_token` (public client).
/// `token_endpoint`/`client_id` come from the stored secret (no re-discovery).
pub(super) async fn refresh(
    client: &Arc<dyn UpstreamClient>,
    secret: &Value,
) -> Result<Value, ChannelError> {
    let token_endpoint = secret_str(secret, "token_endpoint")
        .ok_or_else(|| ChannelError::Build("external_idp secret missing token_endpoint".into()))?;
    let client_id = secret_str(secret, "client_id").unwrap_or_default();
    let refresh_token = secret_str(secret, "refresh_token")
        .ok_or_else(|| ChannelError::InvalidCredential("missing refresh_token".into()))?;

    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ];
    let resp = oauth::token_post(client, token_endpoint, &form, &[]).await?;

    let new_access = resp
        .access_token
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChannelError::Build("refresh response missing access_token".into()))?;
    let expires_at_ms = now_ms() + resp.expires_in.unwrap_or(3600) as i64 * 1000;

    let mut out = secret.clone();
    let obj = out
        .as_object_mut()
        .ok_or_else(|| ChannelError::Build("secret is not an object".into()))?;
    obj.insert("access_token".into(), Value::String(new_access));
    if let Some(rt) = resp.refresh_token.filter(|s| !s.is_empty()) {
        obj.insert("refresh_token".into(), Value::String(rt));
    }
    obj.insert("expires_at_ms".into(), Value::Number(expires_at_ms.into()));
    Ok(out)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn scope_string_appends_offline_access() {
        assert_eq!(scope_string(&json!({})), "openid offline_access");
        assert_eq!(
            scope_string(&json!({ "scopes": "openid profile" })),
            "openid profile offline_access"
        );
        // already present → not duplicated.
        assert_eq!(
            scope_string(&json!({ "scopes": "openid offline_access" })),
            "openid offline_access"
        );
    }

    #[test]
    fn authorize_url_uses_singular_scope_and_s256() {
        let url = authorize_url(
            "https://idp.example.com/authorize",
            "cid-2",
            "http://127.0.0.1:1455/oauth/callback",
            "openid offline_access",
            "st-1",
            "chal-y",
        );
        assert!(url.starts_with("https://idp.example.com/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=cid-2"));
        // singular `scope`, space-joined → %20.
        assert!(url.contains("scope=openid%20offline_access"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=st-1"));
    }

    #[test]
    fn authorize_url_merges_into_existing_query() {
        let url = authorize_url(
            "https://idp.example.com/authorize?foo=bar",
            "cid",
            "http://cb",
            "openid",
            "s",
            "c",
        );
        // existing `?foo=bar` is preserved; our params join with `&`.
        assert!(url.starts_with("https://idp.example.com/authorize?foo=bar&response_type=code"));
        assert!(!url.contains("authorize?foo=bar?"));
    }
}
