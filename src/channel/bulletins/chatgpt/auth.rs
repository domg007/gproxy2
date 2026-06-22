//! Credential refresh orchestration for the chatgpt channel.
//!
//! The secret carries browser-session + anti-bot state
//! (`access_token`/`cookie`/`device_id` from login; `cf_bm`/`chat_req_token`/
//! `proof_token` minted by warmup + sentinel). [`needs_refresh`] decides when the
//! tokens are stale; [`refresh`] re-mints them. The short-lived anti-bot tokens
//! refresh in place; when the ~10-day `access_token` (JWT) nears expiry, the
//! stored `cookie` re-mints a fresh one (via [`exchange`]) so the credential
//! lives as long as the browser session, not the access token.

use std::sync::Arc;

use serde_json::Value;

use super::cookie::{apply_minted, exchange, warmup_capture_cf_bm};
use super::sentinel::{self, run_sentinel};
use crate::channel::ChannelError;
use crate::http::client::UpstreamClient;

/// Refresh the sentinel JWT once it is within this skew of expiry (v1
/// `SENTINEL_REFRESH_SKEW_MS`).
const SENTINEL_REFRESH_SKEW_MS: i64 = 60_000;

/// Re-mint the `access_token` from the stored cookie once the JWT is within this
/// skew of expiry (the access token lasts ~10 days; the session cookie far
/// longer, so this keeps the credential alive without a manual re-paste).
const ACCESS_TOKEN_REFRESH_SKEW_MS: i64 = 60 * 60 * 1000;

/// Whether the anti-bot tokens must be re-minted before the next request: the
/// sentinel token is missing or near expiry, or the `__cf_bm` cookie is missing
/// or past its TTL — or the `access_token` JWT itself is at/near expiry (which
/// triggers a cookie re-mint in [`refresh`]).
pub(super) fn needs_refresh(secret: &Value, now_ms: i64) -> bool {
    // access_token (JWT) at/near expiry → refresh re-mints it from the cookie.
    if let Some(token) = secret.get("access_token").and_then(Value::as_str)
        && let Some(exp) = sentinel::decode_jwt_exp_ms(token)
        && now_ms >= exp - ACCESS_TOKEN_REFRESH_SKEW_MS
    {
        return true;
    }
    let chat_req_token = secret.get("chat_req_token").and_then(Value::as_str);
    if chat_req_token.is_none_or(str::is_empty) {
        return true;
    }
    let expires_at_ms = secret
        .get("chat_req_token_expires_at_ms")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if sentinel::is_expired(expires_at_ms, now_ms, SENTINEL_REFRESH_SKEW_MS) {
        return true;
    }
    let cf_bm = secret.get("cf_bm").and_then(Value::as_str);
    if cf_bm.is_none_or(str::is_empty) {
        return true;
    }
    let cf_bm_expires_at_ms = secret
        .get("cf_bm_expires_at_ms")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    now_ms >= cf_bm_expires_at_ms
}

/// Re-mint stale credential state. When the `access_token` JWT is at/near expiry
/// and the source `cookie` is present, re-exchange the cookie for a fresh secret
/// (new access token + anti-bot), preserving the original `device_id` and any
/// operator-set fields. Otherwise just re-mint the anti-bot tokens (warmup
/// `__cf_bm` + sentinel round) with the existing access token.
pub(super) async fn refresh(
    client: &Arc<dyn UpstreamClient>,
    secret: &Value,
) -> Result<Value, ChannelError> {
    let token = access_token(secret)?.to_string();
    let now_ms = crate::util::time::unix_now_ms() as i64;
    let token_expiring = sentinel::decode_jwt_exp_ms(&token)
        .is_none_or(|exp| now_ms >= exp - ACCESS_TOKEN_REFRESH_SKEW_MS);
    if token_expiring
        && let Some(cookie) = secret
            .get("cookie")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        // Full re-exchange from the cookie → fresh access_token + anti-bot.
        let fresh = exchange(client, cookie).await?;
        let mut out = secret.clone();
        if let (Some(out_obj), Some(fresh_obj)) = (out.as_object_mut(), fresh.as_object()) {
            for (k, v) in fresh_obj {
                // Keep the original device_id (stable per credential); overlay
                // everything else (access_token + cf_bm + sentinel tokens + TTLs).
                if k != "device_id" {
                    out_obj.insert(k.clone(), v.clone());
                }
            }
        }
        return Ok(out);
    }
    let cf_bm = warmup_capture_cf_bm(client, &token).await;
    let tokens = run_sentinel(client, &token).await?;
    let mut out = secret.clone();
    apply_minted(&mut out, cf_bm, tokens);
    Ok(out)
}

/// Read the browser-session `access_token`, erroring if it is missing/empty.
pub(super) fn access_token(secret: &Value) -> Result<&str, ChannelError> {
    secret
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChannelError::InvalidCredential("missing access_token".into()))
}

/// Replace the request headers with the chatgpt-web standard set (Bearer +
/// Edge-147 client-hints + content-type), then overlay the anti-bot tokens
/// carried in the secret: the `__cf_bm` Cloudflare cookie, the sentinel
/// chat-requirements + proof tokens, and the device id.
pub(super) fn apply_request_headers(
    req: &mut http::Request<bytes::Bytes>,
    secret: &Value,
) -> Result<(), ChannelError> {
    let token = access_token(secret)?;
    *req.headers_mut() = super::headers::standard_headers(token);

    let h = req.headers_mut();
    if let Some(cf_bm) = nonempty(secret, "cf_bm") {
        set_header(h, "cookie", &format!("__cf_bm={cf_bm}"))?;
    }
    if let Some(tok) = nonempty(secret, "chat_req_token") {
        set_header(h, "openai-sentinel-chat-requirements-token", tok)?;
    }
    if let Some(tok) = nonempty(secret, "proof_token") {
        set_header(h, "openai-sentinel-proof-token", tok)?;
    }
    if let Some(id) = nonempty(secret, "device_id") {
        set_header(h, "oai-device-id", id)?;
    }
    Ok(())
}

/// Read a non-empty string field from the secret.
fn nonempty<'a>(secret: &'a Value, key: &str) -> Option<&'a str> {
    secret
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Insert a static-named header with a runtime value, mapping a bad value to
/// `InvalidCredential`.
fn set_header(
    headers: &mut http::HeaderMap,
    name: &'static str,
    value: &str,
) -> Result<(), ChannelError> {
    let v = http::HeaderValue::from_str(value)
        .map_err(|e| ChannelError::InvalidCredential(format!("bad {name} header: {e}")))?;
    headers.insert(http::HeaderName::from_static(name), v);
    Ok(())
}
