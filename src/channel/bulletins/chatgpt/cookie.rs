//! Cookie-bootstrap login + `__cf_bm` warmup for the chatgpt channel.
//!
//! Flow (modeled on `claudecode/cookie.rs`): browser session cookie →
//! `GET /api/auth/session` to mint `access_token` → first warmup (capture
//! `__cf_bm`) + sentinel round → merge the anti-bot tokens into the secret.
//! chatgpt.com is Cloudflare-fronted, so the Cloudflare-facing GETs are retried
//! through [`send_ok`] (a fresh connection gets an independent challenge roll).

use std::sync::Arc;

use bytes::Bytes;
use http::header::ACCEPT;
use http::{Request, Response};
use serde_json::Value;

use super::headers::standard_headers;
use super::sentinel::{self, SentinelTokens};
use crate::channel::ChannelError;
use crate::http::client::UpstreamClient;

const SESSION_URL: &str = "https://chatgpt.com/api/auth/session";
const WARMUP_PATHS: &[&str] = &["https://chatgpt.com/", "https://chatgpt.com/backend-api/me"];
/// `__cf_bm` lives ~30min; treat it as good for 25min (v1 `WARMUP_TTL`).
const WARMUP_TTL_MS: i64 = 25 * 60 * 1000;

/// A Cloudflare managed challenge is roughly a per-call coin-flip a non-browser
/// client cannot solve, so the Cloudflare-facing GET is simply retried.
const COOKIE_MAX_ATTEMPTS: u32 = 5;

/// Exchange a browser session `cookie` for the PLAINTEXT secret Value. Mints
/// `access_token` from the session endpoint, then runs the first warmup +
/// sentinel round and folds the anti-bot tokens in. The caller seals + persists.
pub(super) async fn exchange(
    client: &Arc<dyn UpstreamClient>,
    cookie: &str,
) -> Result<Value, ChannelError> {
    let cookie = cookie.trim();
    if cookie.is_empty() {
        return Err(ChannelError::InvalidCredential("empty cookie".into()));
    }

    let body = send_ok(client, "session", || {
        Request::get(SESSION_URL)
            .header(ACCEPT, "application/json")
            .header("cookie", cookie)
            .body(Bytes::new())
            .map_err(|e| ChannelError::Build(format!("session request build: {e}")))
    })
    .await?;
    let session: Value = serde_json::from_slice(&body)
        .map_err(|e| ChannelError::Build(format!("session response parse: {e}")))?;
    let access_token = session
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChannelError::InvalidCredential("session missing accessToken".into()))?
        .to_string();

    let mut secret = serde_json::json!({
        "access_token": access_token,
        "cookie": cookie,
        "device_id": crate::util::rand::uuid_v4(),
    });
    apply_minted(
        &mut secret,
        warmup_capture_cf_bm(client, &access_token).await,
        sentinel::run_sentinel(client, &access_token).await?,
    );
    Ok(secret)
}

/// Overlay the warmup `__cf_bm` + sentinel tokens onto `secret`. Shared by
/// `exchange` and `auth::refresh` so the two paths produce identical fields.
pub(super) fn apply_minted(
    secret: &mut Value,
    cf_bm: Option<(String, i64)>,
    tokens: SentinelTokens,
) {
    let Some(obj) = secret.as_object_mut() else {
        return;
    };
    if let Some((value, expires_at_ms)) = cf_bm {
        obj.insert("cf_bm".into(), Value::String(value));
        obj.insert("cf_bm_expires_at_ms".into(), Value::from(expires_at_ms));
    }
    obj.insert(
        "chat_req_token".into(),
        Value::String(tokens.chat_req_token),
    );
    obj.insert("proof_token".into(), Value::String(tokens.proof_token));
    obj.insert(
        "chat_req_token_expires_at_ms".into(),
        Value::from(tokens.chat_req_token_expires_at_ms),
    );
    if let Some(persona) = tokens.persona {
        obj.insert("persona".into(), Value::String(persona));
    }
}

/// Warmup the Cloudflare edge by hitting `/` then `/backend-api/me`, capturing
/// the `__cf_bm` cookie from any `Set-Cookie`. Best-effort: returns `None`
/// rather than failing if the cookie never appears.
pub(super) async fn warmup_capture_cf_bm(
    client: &Arc<dyn UpstreamClient>,
    access_token: &str,
) -> Option<(String, i64)> {
    let mut cf_bm = None;
    for url in WARMUP_PATHS {
        let mut builder = Request::get(*url);
        if let Some(h) = builder.headers_mut() {
            *h = standard_headers(access_token);
        }
        let Ok(req) = builder.body(Bytes::new()) else {
            continue;
        };
        let Ok(resp) = client.send(req).await else {
            continue;
        };
        if let Some(value) = extract_cf_bm(&resp) {
            cf_bm = Some(value);
        }
    }
    cf_bm.map(|value| {
        (
            value,
            crate::util::time::unix_now_ms() as i64 + WARMUP_TTL_MS,
        )
    })
}

/// Find a `__cf_bm=<value>` among all `Set-Cookie` headers, returning `<value>`
/// (up to the first `;`).
fn extract_cf_bm(resp: &Response<Bytes>) -> Option<String> {
    for raw in resp.headers().get_all(http::header::SET_COOKIE) {
        let Ok(s) = raw.to_str() else { continue };
        if let Some(rest) = s.strip_prefix("__cf_bm=") {
            let value = rest.split(';').next().unwrap_or(rest).to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// Send a Cloudflare-fronted request and return the 2xx body. `build` is invoked
/// once per attempt — `send` consumes the request body, so it must be
/// rebuildable. A managed "Just a moment…" challenge is retried up to
/// [`COOKIE_MAX_ATTEMPTS`] times; any other non-2xx fails at once.
async fn send_ok<F>(
    client: &Arc<dyn UpstreamClient>,
    what: &str,
    build: F,
) -> Result<Bytes, ChannelError>
where
    F: Fn() -> Result<Request<Bytes>, ChannelError>,
{
    let mut last_challenge: Option<(http::StatusCode, Bytes)> = None;
    for attempt in 0..COOKIE_MAX_ATTEMPTS {
        let resp: Response<Bytes> = client
            .send(build()?)
            .await
            .map_err(|e| ChannelError::Build(format!("{what} request failed: {e}")))?;
        let (parts, body) = resp.into_parts();
        if parts.status.is_success() {
            return Ok(body);
        }
        if is_cloudflare_challenge(parts.status, &body) {
            crate::util::time::sleep_ms(200 * (attempt as u64 + 1)).await;
            last_challenge = Some((parts.status, body));
            continue;
        }
        let snippet: String = String::from_utf8_lossy(&body).chars().take(256).collect();
        return Err(ChannelError::Build(format!(
            "{what} endpoint {}: {snippet}",
            parts.status
        )));
    }
    let (status, body) = last_challenge.expect("loop only continues on a challenge");
    let snippet: String = String::from_utf8_lossy(&body).chars().take(160).collect();
    Err(ChannelError::Build(format!(
        "{what} blocked by Cloudflare challenge after {COOKIE_MAX_ATTEMPTS} attempts ({status}): {snippet}"
    )))
}

/// Recognise Cloudflare's interstitial managed-challenge response (the
/// "Just a moment…" page) so it can be retried rather than surfaced as a hard
/// failure. Served as 403 (sometimes 503) with tell-tale challenge HTML.
fn is_cloudflare_challenge(status: http::StatusCode, body: &[u8]) -> bool {
    use http::StatusCode as S;
    if status != S::FORBIDDEN && status != S::SERVICE_UNAVAILABLE {
        return false;
    }
    let head = String::from_utf8_lossy(&body[..body.len().min(1024)]);
    head.contains("Just a moment")
        || head.contains("challenge-platform")
        || head.contains("cf-chl")
        || head.contains("cf_chl")
}
