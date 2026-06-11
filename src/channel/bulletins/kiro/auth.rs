//! Kiro auth — DUAL OAuth `refresh_token` grant.
//!
//! Kiro (Amazon Q / Kiro IDE) ships two login methods, distinguished by the
//! decrypted secret shape:
//!   * **social** (Google / GitHub via the Kiro desktop portal) — refresh hits
//!     `{auth_base}/refreshToken` with `{"refreshToken": rt}`.
//!   * **IdC** (AWS Identity Center / Builder ID, an OIDC client registration) —
//!     refresh hits `https://oidc.{region}.amazonaws.com/token` with
//!     `{clientId, clientSecret, refreshToken, grantType:"refresh_token"}`.
//!
//! Discriminator: `client_id` + `client_secret` present → IdC, else social.
//! Both endpoints take a JSON body (NOT form-urlencoded), so the shared
//! [`oauth::token_post`](crate::channel::oauth::token_post) form helper does not
//! fit — [`json_post`] posts a JSON body via the same [`UpstreamClient`] and
//! parses the camelCase token response.
//!
//! The login-time PKCE authorize/exchange + OIDC client registration are an M10
//! concern; this module covers the per-request bearer + the refresh the pipeline
//! drives. Refresh maps camelCase → secret fields, rotates `refresh_token` when
//! the response carries one, recomputes `expires_at_ms` from `expiresIn`, and
//! stores `profile_arn` when returned (else preserves the existing one).

use std::sync::Arc;

use bytes::Bytes;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::channel::ChannelError;
use crate::http::client::UpstreamClient;

/// Default Kiro desktop auth host (social refresh + portal login).
pub(super) const DEFAULT_AUTH_BASE_URL: &str = "https://prod.us-east-1.auth.desktop.kiro.dev";
/// Default Kiro portal host hosting the social `/signin` consent page (mined
/// from v1 `default_kiro_auth_portal_url`).
pub(super) const DEFAULT_PORTAL_URL: &str = "https://app.kiro.dev";
/// Default redirect_uri the social login uses when the caller passes none
/// (mined from v1 `default_kiro_oauth_redirect_uri`) — a loopback listener.
pub(super) const DEFAULT_REDIRECT_URI: &str = "http://localhost:3128";
/// Kiro IDE user-agent the auth endpoints key behaviour off.
const AUTH_USER_AGENT: &str = "KiroIDE-0.12.224-gproxy";
/// Refresh slightly before expiry to avoid racing a 401 mid-flight.
const EXPIRY_SKEW_MS: i64 = 60_000;

/// Read a trimmed, non-empty string field from the secret.
fn secret_str<'a>(secret: &'a Value, key: &str) -> Option<&'a str> {
    secret
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The Kiro access token (Bearer), required by [`super::KiroChannel::prepare`].
pub(super) fn access_token(secret: &Value) -> Result<&str, ChannelError> {
    secret_str(secret, "access_token")
        .ok_or_else(|| ChannelError::InvalidCredential("missing access_token".into()))
}

/// The CodeWhisperer `profile_arn`, lifted into the Smithy body when present
/// (secret takes precedence over the provider default).
pub(super) fn profile_arn<'a>(secret: &'a Value, settings: &'a Value) -> Option<&'a str> {
    secret_str(secret, "profile_arn").or_else(|| secret_str(settings, "profile_arn"))
}

/// IdC when both `client_id` and `client_secret` are present, else social.
fn is_idc(secret: &Value) -> bool {
    secret_str(secret, "client_id").is_some() && secret_str(secret, "client_secret").is_some()
}

/// Percent-encode a query value, leaving the RFC 3986 unreserved set verbatim.
fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(
                char::from_digit((b >> 4) as u32, 16)
                    .unwrap()
                    .to_ascii_uppercase(),
            );
            out.push(
                char::from_digit((b & 0xf) as u32, 16)
                    .unwrap()
                    .to_ascii_uppercase(),
            );
        }
    }
    out
}

/// Build the SOCIAL authorize URL (`{portal}/signin?...`) for the interactive
/// authcode+PKCE login. An empty `redirect_uri` falls back to
/// [`DEFAULT_REDIRECT_URI`]. The query carries the recorded social params
/// (`state`, `code_challenge` + S256, `redirect_uri`, `redirect_from=KiroIDE`),
/// mined from v1 `build_kiro_portal_authorize_url`.
///
/// IdC (AWS OIDC client registration) login is OUT of scope here — it requires a
/// `RegisterClient` round-trip before the authorize step, so only the social
/// authcode flow is wired.
pub(super) fn authcode_start(redirect_uri: &str, state: &str, challenge: &str) -> (String, String) {
    let redirect_uri = if redirect_uri.trim().is_empty() {
        DEFAULT_REDIRECT_URI
    } else {
        redirect_uri
    };
    let query = [
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("redirect_uri", redirect_uri),
        ("redirect_from", "KiroIDE"),
    ]
    .iter()
    .map(|(k, v)| format!("{k}={}", pct(v)))
    .collect::<Vec<_>>()
    .join("&");
    let portal = DEFAULT_PORTAL_URL.trim_end_matches('/');
    (format!("{portal}/signin?{query}"), redirect_uri.to_string())
}

/// Social `/oauth/token` exchange response — snake_case (distinct from the
/// camelCase [`TokenResponse`] the refresh endpoints return).
#[derive(Debug, Deserialize)]
struct SocialTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    profile_arn: Option<String>,
    expires_in: Option<u64>,
}

/// Exchange a social authcode (+PKCE verifier) for the plaintext secret. Kiro's
/// `{auth_base}/oauth/token` takes a JSON body (NOT form-urlencoded) and returns
/// snake_case tokens. Maps to `{access_token, refresh_token, profile_arn?,
/// expires_at_ms, auth_method:"social", provider:"social"}`.
pub(super) async fn authcode_exchange(
    client: &Arc<dyn UpstreamClient>,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<Value, ChannelError> {
    let url = format!(
        "{}/oauth/token",
        DEFAULT_AUTH_BASE_URL.trim_end_matches('/')
    );
    let body = json!({
        "code": code,
        "code_verifier": verifier,
        "redirect_uri": redirect_uri,
    });
    let resp: SocialTokenResponse = json_post(client, &url, &body).await?;

    let access_token = resp
        .access_token
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("token response missing accessToken".into()))?;
    let refresh_token = resp
        .refresh_token
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("token response missing refreshToken".into()))?;
    let expires_at_ms = crate::util::time::unix_now().saturating_mul(1000)
        + resp.expires_in.unwrap_or(3600) as i64 * 1000;

    let mut secret = json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "expires_at_ms": expires_at_ms,
        "auth_method": "social",
        "provider": "social",
    });
    if let Some(arn) = resp.profile_arn.filter(|s| !s.trim().is_empty()) {
        secret["profile_arn"] = Value::String(arn);
    }
    Ok(secret)
}

/// Whether the access token is absent or within the skew window of expiry.
pub(super) fn needs_refresh(secret: &Value) -> bool {
    if secret_str(secret, "access_token").is_none() {
        return true;
    }
    let expires_at_ms = secret
        .get("expires_at_ms")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    // `expires_at_ms == 0` means "unknown" → treat as valid; the 401-driven
    // refresh path still covers stale tokens.
    if expires_at_ms == 0 {
        return false;
    }
    let now_ms = crate::util::time::unix_now().saturating_mul(1000);
    now_ms > expires_at_ms - EXPIRY_SKEW_MS
}

/// Kiro token-endpoint response (both social + IdC share this camelCase shape).
/// Tolerant: every field optional so a refresh that omits `refreshToken` (reuse
/// the old one) or `profileArn` still parses.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    profile_arn: Option<String>,
    expires_in: Option<u64>,
}

/// Refresh the credential, dispatching on the social/IdC discriminator. Returns
/// the new plaintext secret with `access_token`/`expires_at_ms` rotated,
/// `refresh_token` + `profile_arn` rotated when present (else preserved), and
/// every other field carried through the clone.
pub(super) async fn refresh(
    client: &Arc<dyn UpstreamClient>,
    settings: &Value,
    secret: &Value,
) -> Result<Value, ChannelError> {
    let refresh_token = secret_str(secret, "refresh_token")
        .ok_or_else(|| ChannelError::InvalidCredential("missing refresh_token".into()))?
        .to_string();

    let (url, body) = if is_idc(secret) {
        let region = secret_str(secret, "region").unwrap_or("us-east-1");
        let client_id = secret_str(secret, "client_id").unwrap_or_default();
        let client_secret = secret_str(secret, "client_secret").unwrap_or_default();
        (
            format!("https://oidc.{region}.amazonaws.com/token"),
            json!({
                "clientId": client_id,
                "clientSecret": client_secret,
                "refreshToken": refresh_token,
                "grantType": "refresh_token",
            }),
        )
    } else {
        let auth_base = secret_str(settings, "auth_base_url").unwrap_or(DEFAULT_AUTH_BASE_URL);
        (
            format!("{}/refreshToken", auth_base.trim_end_matches('/')),
            json!({ "refreshToken": refresh_token }),
        )
    };

    let resp: TokenResponse = json_post(client, &url, &body).await?;

    let new_access = resp
        .access_token
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("refresh response missing accessToken".into()))?;
    let expires_at_ms = crate::util::time::unix_now().saturating_mul(1000)
        + resp.expires_in.unwrap_or(3600) as i64 * 1000;

    let mut out = secret.clone();
    let obj = out
        .as_object_mut()
        .ok_or_else(|| ChannelError::Build("secret is not an object".into()))?;
    obj.insert("access_token".into(), Value::String(new_access));
    // refresh_token ROTATES when present — store the new one, else keep the old.
    if let Some(rt) = resp.refresh_token.filter(|s| !s.trim().is_empty()) {
        obj.insert("refresh_token".into(), Value::String(rt));
    }
    // profile_arn is returned only by some refreshes — store it, else preserve.
    if let Some(arn) = resp.profile_arn.filter(|s| !s.trim().is_empty()) {
        obj.insert("profile_arn".into(), Value::String(arn));
    }
    obj.insert("expires_at_ms".into(), Value::Number(expires_at_ms.into()));
    Ok(out)
}

/// POST a JSON `body` to `url` and parse the [`TokenResponse`]. Mirrors
/// [`oauth::token_post`](crate::channel::oauth::token_post) but with a JSON body
/// (the Kiro/OIDC token endpoints reject form-urlencoded). Rides the passed
/// [`UpstreamClient`] (proxy pool / edge transport). Non-2xx →
/// [`ChannelError::Build`] with the status + (truncated) body.
async fn json_post<T: serde::de::DeserializeOwned>(
    client: &Arc<dyn UpstreamClient>,
    url: &str,
    body: &Value,
) -> Result<T, ChannelError> {
    let payload = serde_json::to_vec(body)
        .map_err(|e| ChannelError::Build(format!("refresh request serialize: {e}")))?;
    let req = http::Request::builder()
        .method(http::Method::POST)
        .uri(url)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json")
        .header(http::header::USER_AGENT, AUTH_USER_AGENT)
        .body(Bytes::from(payload))
        .map_err(|e| ChannelError::Build(format!("refresh request build: {e}")))?;

    let resp = client
        .send(req)
        .await
        .map_err(|e| ChannelError::Build(format!("refresh request failed: {e}")))?;
    let (parts, body) = resp.into_parts();
    if !parts.status.is_success() {
        let snippet: String = String::from_utf8_lossy(&body).chars().take(256).collect();
        return Err(ChannelError::Build(format!(
            "refresh endpoint {}: {snippet}",
            parts.status
        )));
    }
    serde_json::from_slice(&body)
        .map_err(|e| ChannelError::Build(format!("refresh response parse: {e}")))
}
