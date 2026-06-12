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
//! Login covers both methods: [`authcode_start`]/[`authcode_exchange`] for the
//! social portal flow, and [`idc_authcode_start`]/[`idc_authcode_exchange`] for
//! AWS SSO-OIDC (dynamic `RegisterClient` → authorize → token). Refresh maps
//! camelCase → secret fields, rotates `refresh_token` when the response carries
//! one, recomputes `expires_at_ms` from `expiresIn`, and stores `profile_arn`
//! when returned (else preserves the existing one).

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
const AUTH_USER_AGENT: &str = "KiroIDE-0.12.224";
/// Refresh slightly before expiry to avoid racing a 401 mid-flight.
const EXPIRY_SKEW_MS: i64 = 60_000;

/// AWS SSO-OIDC IdC login constants (ported from v1 `kiro.rs`).
const IDC_DEFAULT_REDIRECT_URI: &str = "http://127.0.0.1/oauth/callback";
const BUILDER_ID_START_URL: &str = "https://view.awsapps.com/start";
const INTERNAL_SSO_START_URL: &str = "https://amzn.awsapps.com/start";
/// Grant scopes requested at register/authorize; sent as `{prefix}:{scope}`.
const IDC_GRANT_SCOPES: &[&str] = &[
    "completions",
    "analysis",
    "conversations",
    "transformations",
    "taskassist",
];
const IDC_DEFAULT_SCOPE_PREFIX: &str = "codewhisperer";

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
/// This is the SOCIAL flow (static authorize URL). The IdC (AWS OIDC) flow needs
/// an async `RegisterClient` round-trip first — see [`idc_authcode_start`].
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

// ── IdC (AWS SSO-OIDC) login (ported from v1 `kiro.rs`) ──────────────────────

/// OIDC base host for a region (matches the IdC refresh endpoint).
fn oidc_base(region: &str) -> String {
    format!("https://oidc.{}.amazonaws.com", region.trim())
}

/// Whether the operator requested the IdC flow: `auth_method`/`login_option` is
/// an IdC alias, or a `start_url` was supplied.
pub(super) fn idc_requested(params: &Value) -> bool {
    let method = secret_str(params, "auth_method")
        .or_else(|| secret_str(params, "login_option"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        method.as_str(),
        "idc" | "iam_sso" | "awsidc" | "builderid" | "internal"
    ) || secret_str(params, "start_url").is_some()
}

/// Provider label: `BuilderId` | `Internal` | `Enterprise` (default).
fn idc_provider(params: &Value) -> String {
    let value = secret_str(params, "provider")
        .or_else(|| secret_str(params, "login_option"))
        .or_else(|| secret_str(params, "auth_provider"))
        .unwrap_or("Enterprise");
    match value.to_ascii_lowercase().as_str() {
        "builderid" | "builder_id" | "builder" => "BuilderId".to_string(),
        "internal" => "Internal".to_string(),
        _ => "Enterprise".to_string(),
    }
}

/// IdC issuer/start URL: explicit `start_url`/`issuer_url`, else the per-provider
/// default. Enterprise/AWS IdC has no default and requires one.
fn idc_start_url(params: &Value, provider: &str) -> Result<String, ChannelError> {
    if let Some(url) = secret_str(params, "start_url").or_else(|| secret_str(params, "issuer_url"))
    {
        return Ok(url.to_string());
    }
    match provider {
        "BuilderId" => Ok(BUILDER_ID_START_URL.to_string()),
        "Internal" => Ok(INTERNAL_SSO_START_URL.to_string()),
        _ => Err(ChannelError::Build(
            "kiro idc login requires start_url for Enterprise/AWS IdC".into(),
        )),
    }
}

/// Grant scopes as `{prefix}:{scope}` (prefix defaults to `codewhisperer`).
fn idc_scopes(params: &Value) -> Vec<String> {
    let prefix = secret_str(params, "scope_prefix").unwrap_or(IDC_DEFAULT_SCOPE_PREFIX);
    IDC_GRANT_SCOPES
        .iter()
        .map(|scope| format!("{prefix}:{scope}"))
        .collect()
}

/// AWS SSO-OIDC `RegisterClient` response (`clientId`/`clientSecret`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OidcRegisterResponse {
    client_id: Option<String>,
    client_secret: Option<String>,
}

/// `POST {oidc}/client/register`: dynamically register a public OIDC client.
/// Returns `(client_id, client_secret)`.
async fn register_oidc_client(
    client: &Arc<dyn UpstreamClient>,
    region: &str,
    start_url: &str,
    redirect_uri: &str,
    scopes: &[String],
) -> Result<(String, String), ChannelError> {
    let url = format!("{}/client/register", oidc_base(region));
    let body = json!({
        "clientName": "Kiro IDE",
        "clientType": "public",
        "scopes": scopes,
        "grantTypes": ["authorization_code", "refresh_token"],
        "redirectUris": [redirect_uri],
        "issuerUrl": start_url,
    });
    let resp: OidcRegisterResponse = json_post(client, &url, &body).await?;
    let client_id = resp
        .client_id
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("oidc register missing clientId".into()))?;
    let client_secret = resp
        .client_secret
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("oidc register missing clientSecret".into()))?;
    Ok((client_id, client_secret))
}

/// Build the IdC authorize URL (`{oidc}/authorize?...`), scopes comma-joined.
fn build_idc_authorize_url(
    region: &str,
    client_id: &str,
    redirect_uri: &str,
    scopes: &[String],
    state: &str,
    challenge: &str,
) -> String {
    let scope_param = scopes.join(",");
    let query = [
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scopes", scope_param.as_str()),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ]
    .iter()
    .map(|(k, v)| format!("{k}={}", pct(v)))
    .collect::<Vec<_>>()
    .join("&");
    format!("{}/authorize?{query}", oidc_base(region))
}

/// IdC authorize start: register a dynamic client, build the authorize URL, and
/// return `(authorize_url, redirect_uri, extra)` where `extra` carries the
/// registered client creds for the later exchange. An empty `redirect_uri` falls
/// back to [`IDC_DEFAULT_REDIRECT_URI`].
pub(super) async fn idc_authcode_start(
    client: &Arc<dyn UpstreamClient>,
    params: &Value,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> Result<(String, String, Value), ChannelError> {
    let region = secret_str(params, "region")
        .unwrap_or("us-east-1")
        .to_string();
    let provider = idc_provider(params);
    let start_url = idc_start_url(params, &provider)?;
    let redirect = if redirect_uri.trim().is_empty() {
        IDC_DEFAULT_REDIRECT_URI.to_string()
    } else {
        redirect_uri.trim().to_string()
    };
    let scopes = idc_scopes(params);
    let (client_id, client_secret) =
        register_oidc_client(client, &region, &start_url, &redirect, &scopes).await?;
    let authorize_url =
        build_idc_authorize_url(&region, &client_id, &redirect, &scopes, state, challenge);
    let extra = json!({
        "client_id": client_id,
        "client_secret": client_secret,
        "region": region,
        "provider": provider,
    });
    Ok((authorize_url, redirect, extra))
}

/// IdC code exchange using the creds stashed at start (`extra`). Maps to the
/// secret `{access_token, refresh_token, profile_arn?, expires_at_ms,
/// auth_method:"IdC", provider, client_id, client_secret, region}`.
pub(super) async fn idc_authcode_exchange(
    client: &Arc<dyn UpstreamClient>,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    extra: &Value,
) -> Result<Value, ChannelError> {
    let client_id = secret_str(extra, "client_id")
        .ok_or_else(|| ChannelError::Build("idc exchange: missing client_id".into()))?;
    let client_secret = secret_str(extra, "client_secret")
        .ok_or_else(|| ChannelError::Build("idc exchange: missing client_secret".into()))?;
    let region = secret_str(extra, "region").unwrap_or("us-east-1");
    let provider = secret_str(extra, "provider").unwrap_or("Enterprise");

    let url = format!("{}/token", oidc_base(region));
    let body = json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "grantType": "authorization_code",
        "redirectUri": redirect_uri,
        "code": code,
        "codeVerifier": verifier,
    });
    let resp: TokenResponse = json_post(client, &url, &body).await?;

    let access_token = resp
        .access_token
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("idc token response missing accessToken".into()))?;
    let refresh_token = resp
        .refresh_token
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("idc token response missing refreshToken".into()))?;
    let expires_at_ms = crate::util::time::unix_now().saturating_mul(1000)
        + resp.expires_in.unwrap_or(3600) as i64 * 1000;

    let mut secret = json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "expires_at_ms": expires_at_ms,
        "auth_method": "IdC",
        "provider": provider,
        "client_id": client_id,
        "client_secret": client_secret,
        "region": region,
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
        .map_err(|e| ChannelError::Build(format!("kiro auth request serialize: {e}")))?;
    let req = http::Request::builder()
        .method(http::Method::POST)
        .uri(url)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json")
        .header(http::header::USER_AGENT, AUTH_USER_AGENT)
        .body(Bytes::from(payload))
        .map_err(|e| ChannelError::Build(format!("kiro auth request build: {e}")))?;

    let resp = client
        .send(req)
        .await
        .map_err(|e| ChannelError::Build(format!("kiro auth request failed: {e}")))?;
    let (parts, body) = resp.into_parts();
    if !parts.status.is_success() {
        let snippet: String = String::from_utf8_lossy(&body).chars().take(256).collect();
        return Err(ChannelError::Build(format!(
            "kiro auth endpoint {}: {snippet}",
            parts.status
        )));
    }
    serde_json::from_slice(&body)
        .map_err(|e| ChannelError::Build(format!("kiro auth response parse: {e}")))
}
