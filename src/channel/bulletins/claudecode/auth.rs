//! Claude Code auth — Anthropic OAuth2 `refresh_token` grant + the
//! claude-cli / `@anthropic-ai/sdk` impersonation header set. Base
//! `https://api.anthropic.com`; token endpoint `/v1/oauth/token`. A
//! session-cookie bootstrap (claude.ai → token exchange) is a documented
//! follow-up (see [`refresh`]).
//!
//! As an impersonation channel it forwards the claude-cli fingerprint headers
//! (its per-channel allow-list, applied after the global blacklist):
//! `user-agent`, `anthropic-beta`, `anthropic-dangerous-direct-browser-access`,
//! `x-app`, `x-claude-code-session-id`, and the `x-stainless-*` family
//! (arch / lang / os / package-version / retry-count / runtime /
//! runtime-version / timeout). `anthropic-version` is injected, not forwarded.

use std::sync::Arc;

use bytes::Bytes;
use http::Request;
use http::header::{AUTHORIZATION, HeaderName, HeaderValue};
use serde_json::Value;

use crate::channel::ChannelError;
use crate::channel::oauth;
use crate::http::client::UpstreamClient;

pub(super) const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub(super) const TOKEN_URL: &str = "https://api.anthropic.com/v1/oauth/token";
pub(super) const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

const ANTHROPIC_VERSION: &str = "2023-06-01";
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";
const USER_AGENT: &str = "claude-cli/2.1.154 (external, cli)";

/// Refresh slightly before expiry to avoid racing a 401 mid-flight.
const EXPIRY_SKEW_MS: i64 = 60_000;

/// Anthropic JS SDK (Stainless-generated) default header values, mirroring real
/// `claude-cli` 2.1.154 / `@anthropic-ai/sdk` 0.81.0 traffic. Injected verbatim;
/// per-credential overrides are an M7a fingerprint-pool concern.
const STAINLESS: &[(&str, &str)] = &[
    ("x-stainless-retry-count", "0"),
    ("x-stainless-timeout", "600"),
    ("x-stainless-lang", "js"),
    ("x-stainless-package-version", "0.81.0"),
    ("x-stainless-os", "Linux"),
    ("x-stainless-arch", "x64"),
    ("x-stainless-runtime", "node"),
    ("x-stainless-runtime-version", "v22.20.0"),
];

/// Read a trimmed, non-empty string field from the secret.
fn secret_str<'a>(secret: &'a Value, key: &str) -> Option<&'a str> {
    secret
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The OAuth access token, required by [`super::ClaudeCodeChannel::prepare`].
pub(super) fn access_token(secret: &Value) -> Result<&str, ChannelError> {
    secret_str(secret, "access_token")
        .ok_or_else(|| ChannelError::InvalidCredential("missing access_token".into()))
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

/// Refresh via the Anthropic `refresh_token` grant, returning the new plaintext
/// secret (both tokens rotate; `expires_at_ms` is recomputed; cookie /
/// account_uuid / device_id / user_email are preserved).
///
/// Anthropic's `/v1/oauth/token` rejects refreshes that omit `client_id` or the
/// `anthropic-version` / `anthropic-beta` / CLI `user-agent` headers, so we send
/// them explicitly via [`oauth::token_post`].
///
/// Cookie fallback (M7b follow-up): a credential carrying only a `cookie` (no
/// `refresh_token`) requires the multi-step claude.ai → org-discovery → token
/// exchange bootstrap. That is deferred — such a credential errors here.
pub(super) async fn refresh(
    client: &Arc<dyn UpstreamClient>,
    secret: &Value,
) -> Result<Value, ChannelError> {
    let refresh_token = match secret_str(secret, "refresh_token") {
        Some(rt) => rt,
        None if secret_str(secret, "cookie").is_some() => {
            return Err(ChannelError::Unsupported(
                "cookie login not yet implemented (M7b follow-up)",
            ));
        }
        None => {
            return Err(ChannelError::InvalidCredential(
                "missing refresh_token".into(),
            ));
        }
    };

    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", OAUTH_CLIENT_ID),
        ("refresh_token", refresh_token),
    ];
    let extra_headers = [
        ("anthropic-version", ANTHROPIC_VERSION),
        ("anthropic-beta", ANTHROPIC_BETA),
        ("user-agent", USER_AGENT),
    ];
    let resp = oauth::token_post(client, TOKEN_URL, &form, &extra_headers).await?;

    let new_access = resp
        .access_token
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChannelError::Build("refresh response missing access_token".into()))?;
    let expires_at_ms = crate::util::time::unix_now().saturating_mul(1000)
        + resp.expires_in.unwrap_or(3600) as i64 * 1000;

    let mut out = secret.clone();
    let obj = out
        .as_object_mut()
        .ok_or_else(|| ChannelError::Build("secret is not an object".into()))?;
    obj.insert("access_token".into(), Value::String(new_access));
    // refresh_token ROTATES — store the new one when present, else keep the old.
    if let Some(rt) = resp.refresh_token.filter(|s| !s.is_empty()) {
        obj.insert("refresh_token".into(), Value::String(rt));
    }
    obj.insert("expires_at_ms".into(), Value::Number(expires_at_ms.into()));
    Ok(out)
}

/// Inject the OAuth bearer + claude-cli / Stainless impersonation headers onto
/// the prepared upstream request. A per-request session-id is generated.
pub(super) fn apply(req: &mut Request<Bytes>, access_token: &str) -> Result<(), ChannelError> {
    let bearer = HeaderValue::from_str(&format!("Bearer {access_token}"))
        .map_err(|e| ChannelError::InvalidCredential(format!("bad access_token: {e}")))?;
    let session_id = HeaderValue::from_str(&new_session_id())
        .map_err(|e| ChannelError::Build(format!("bad session id: {e}")))?;

    let h = req.headers_mut();
    h.insert(AUTHORIZATION, bearer);
    h.insert(
        HeaderName::from_static("anthropic-version"),
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );
    h.insert(
        HeaderName::from_static("anthropic-beta"),
        HeaderValue::from_static(ANTHROPIC_BETA),
    );
    h.insert(
        HeaderName::from_static("anthropic-dangerous-direct-browser-access"),
        HeaderValue::from_static("true"),
    );
    h.insert(
        HeaderName::from_static("x-app"),
        HeaderValue::from_static("cli"),
    );
    h.insert(
        HeaderName::from_static("x-claude-code-session-id"),
        session_id,
    );
    h.insert(
        http::header::USER_AGENT,
        HeaderValue::from_static(USER_AGENT),
    );
    for (name, value) in STAINLESS {
        h.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    Ok(())
}

/// Fresh per-request v4 session id. `uuid` is a native-only dependency (Cargo
/// target tables), so wasm falls back to a JS-clock + counter id — the same
/// split as [`crate::channel::bulletins::copilot_cli`].
#[cfg(not(target_arch = "wasm32"))]
fn new_session_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(target_arch = "wasm32")]
fn new_session_id() -> String {
    use core::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{:x}", js_sys::Date::now() as u64, n)
}
