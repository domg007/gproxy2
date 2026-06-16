//! Kiro auth — device-code login + DUAL OAuth `refresh_token` grant.
//!
//! Login is a CAPTURED device-code flow against the Kiro desktop auth host
//! (`kiro-cli-chat`): [`device_start`] asks for a device + user code, the
//! operator authorizes in a browser, and [`device_poll`] returns the plaintext
//! secret once authorized. Device tokens carry no expiry; refresh kicks in via
//! [`refresh`].
//!
//! Refresh distinguishes two secret shapes:
//!   * **device / social** (GitHub via the Kiro desktop portal) — refresh hits
//!     `{auth_base}/refreshToken` with `{"refreshToken": rt}`.
//!   * **IdC** (AWS Identity Center / Builder ID, an OIDC client registration) —
//!     refresh hits `https://oidc.{region}.amazonaws.com/token` with
//!     `{clientId, clientSecret, refreshToken, grantType:"refresh_token"}`.
//!
//! Discriminator: `client_id` + `client_secret` present → IdC, else
//! device/social. Both endpoints take a JSON body (NOT form-urlencoded), so the
//! shared [`oauth::token_post`](crate::channel::oauth::token_post) form helper
//! does not fit — [`json_post`] posts a JSON body via the same
//! [`UpstreamClient`] and parses the camelCase token response.

use std::sync::Arc;

use bytes::Bytes;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::channel::ChannelError;
use crate::channel::login::{DeviceInit, DevicePoll};
use crate::http::client::UpstreamClient;

/// Default Kiro desktop auth host (device + social refresh + device login).
pub(super) const DEFAULT_AUTH_BASE_URL: &str = "https://prod.us-east-1.auth.desktop.kiro.dev";
/// User-agent the captured `kiro-cli-chat` device requests send; the auth
/// endpoints key behaviour off it. Used by [`json_post`] for ALL auth calls.
const AUTH_USER_AGENT: &str = "Kiro-CLI";
/// Refresh slightly before expiry to avoid racing a 401 mid-flight.
const EXPIRY_SKEW_MS: i64 = 60_000;
/// Literal `clientId` the device flow sends (there is NO client_secret).
const DEVICE_CLIENT_ID: &str = "Kiro-CLI";
/// Login provider the device flow requests (GitHub via the Kiro portal).
const DEVICE_LOGIN_PROVIDER: &str = "Github";
/// Device tokens carry no expiry; assume a 1h access-token life so the
/// proactive refresh path has an `expires_at_ms` to compare against.
const DEVICE_TOKEN_TTL_MS: i64 = 3_600_000;

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

/// IdC when both `client_id` and `client_secret` are present, else device/social.
fn is_idc(secret: &Value) -> bool {
    secret_str(secret, "client_id").is_some() && secret_str(secret, "client_secret").is_some()
}

/// Build the device-authorization request body: the captured `kiro-cli-chat`
/// posts `{"clientId":"Kiro-CLI","loginProvider":"Github"}`.
fn device_start_body() -> Value {
    json!({
        "clientId": DEVICE_CLIENT_ID,
        "loginProvider": DEVICE_LOGIN_PROVIDER,
    })
}

/// `POST {auth_base}/oauth/device/authorization` response (camelCase).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceAuthorizationResponse {
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri_complete: Option<String>,
    verification_uri: Option<String>,
    interval_in_milliseconds: Option<u64>,
}

/// Begin a device-code login: `POST {auth_base}/oauth/device/authorization` with
/// `{"clientId":"Kiro-CLI","loginProvider":"Github"}`. Maps the response to a
/// [`DeviceInit`] (verification URL prefers the `complete` form; interval is the
/// provider's milliseconds / 1000, floored to 1s, defaulting to 5s).
pub(super) async fn device_start(
    client: &Arc<dyn UpstreamClient>,
) -> Result<DeviceInit, ChannelError> {
    let url = format!(
        "{}/oauth/device/authorization",
        DEFAULT_AUTH_BASE_URL.trim_end_matches('/')
    );
    let resp: DeviceAuthorizationResponse = json_post(client, &url, &device_start_body()).await?;

    let device_code = resp
        .device_code
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("device authorization missing deviceCode".into()))?;
    let user_code = resp
        .user_code
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("device authorization missing userCode".into()))?;
    let verification_url = resp
        .verification_uri_complete
        .or(resp.verification_uri)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            ChannelError::Build("device authorization missing verificationUri".into())
        })?;
    let interval_secs = resp
        .interval_in_milliseconds
        .map(|ms| (ms / 1000).max(1))
        .unwrap_or(5);

    Ok(DeviceInit {
        device_code,
        user_code,
        verification_url,
        interval_secs,
    })
}

/// `POST {auth_base}/oauth/device/poll` response (camelCase).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevicePollResponse {
    status: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    profile_arn: Option<String>,
    identity_provider: Option<String>,
}

/// Build the device-poll request body: `{"deviceCode":<code>,"clientId":"Kiro-CLI"}`.
fn device_poll_body(device_code: &str) -> Value {
    json!({
        "deviceCode": device_code,
        "clientId": DEVICE_CLIENT_ID,
    })
}

/// Poll a pending device-code login. `authorization_pending`/`slow_down` →
/// [`DevicePoll::Pending`]; `authorized` → [`DevicePoll::Ready`] with the
/// plaintext secret; anything else (expired/denied) → [`DevicePoll::Denied`].
pub(super) async fn device_poll(
    client: &Arc<dyn UpstreamClient>,
    device_code: &str,
) -> Result<DevicePoll, ChannelError> {
    let url = format!(
        "{}/oauth/device/poll",
        DEFAULT_AUTH_BASE_URL.trim_end_matches('/')
    );
    let resp: DevicePollResponse = json_post(client, &url, &device_poll_body(device_code)).await?;

    match resp.status.as_deref() {
        Some("authorization_pending") | Some("slow_down") => Ok(DevicePoll::Pending),
        Some("authorized") => Ok(DevicePoll::Ready(device_secret(resp)?)),
        _ => Ok(DevicePoll::Denied),
    }
}

/// Shape an `authorized` poll response into the plaintext secret the caller
/// seals + persists. `profile_arn` is omitted when absent; `provider` defaults
/// to `Github`; `expires_at_ms` is set an hour out (device tokens lack expiry).
fn device_secret(resp: DevicePollResponse) -> Result<Value, ChannelError> {
    let access_token = resp
        .access_token
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("device poll missing accessToken".into()))?;
    let refresh_token = resp
        .refresh_token
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ChannelError::Build("device poll missing refreshToken".into()))?;
    let provider = resp
        .identity_provider
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEVICE_LOGIN_PROVIDER.to_string());
    let expires_at_ms = crate::util::time::unix_now().saturating_mul(1000) + DEVICE_TOKEN_TTL_MS;

    let mut secret = json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "provider": provider,
        "expires_at_ms": expires_at_ms,
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

/// Kiro token-endpoint response (both social + IdC refresh share this camelCase
/// shape). Tolerant: every field optional so a refresh that omits `refreshToken`
/// (reuse the old one) or `profileArn` still parses.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    profile_arn: Option<String>,
    expires_in: Option<u64>,
}

/// Refresh the credential, dispatching on the device-social/IdC discriminator.
/// Returns the new plaintext secret with `access_token`/`expires_at_ms` rotated,
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

/// POST a JSON `body` to `url` and parse the response. Mirrors
/// [`oauth::token_post`](crate::channel::oauth::token_post) but with a JSON body
/// (the Kiro/OIDC endpoints reject form-urlencoded). Rides the passed
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn device_start_body_matches_capture() {
        // The captured kiro-cli-chat device-authorization request body.
        let body = device_start_body();
        assert_eq!(body["clientId"], "Kiro-CLI");
        assert_eq!(body["loginProvider"], "Github");
        assert_eq!(
            body.as_object().unwrap().len(),
            2,
            "no extra fields beyond clientId + loginProvider"
        );
    }

    #[test]
    fn device_poll_body_carries_device_code_and_client_id() {
        let body = device_poll_body("dev-code-xyz");
        assert_eq!(body["deviceCode"], "dev-code-xyz");
        assert_eq!(body["clientId"], "Kiro-CLI");
    }

    #[test]
    fn device_secret_maps_authorized_response() {
        let resp: DevicePollResponse = serde_json::from_value(json!({
            "status": "authorized",
            "accessToken": "at-1",
            "refreshToken": "rt-1",
            "profileArn": "arn:aws:kiro:profile/p1",
            "identityProvider": "Github",
        }))
        .unwrap();
        let secret = device_secret(resp).unwrap();
        assert_eq!(secret["access_token"], "at-1");
        assert_eq!(secret["refresh_token"], "rt-1");
        assert_eq!(secret["profile_arn"], "arn:aws:kiro:profile/p1");
        assert_eq!(secret["provider"], "Github");
        assert!(secret["expires_at_ms"].as_i64().unwrap() > crate::util::time::unix_now() * 1000);
    }

    #[test]
    fn device_secret_omits_absent_profile_arn_and_defaults_provider() {
        let resp: DevicePollResponse = serde_json::from_value(json!({
            "status": "authorized",
            "accessToken": "at-2",
            "refreshToken": "rt-2",
        }))
        .unwrap();
        let secret = device_secret(resp).unwrap();
        assert!(secret.get("profile_arn").is_none());
        assert_eq!(secret["provider"], "Github");
    }
}
