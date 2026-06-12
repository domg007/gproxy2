//! Claude Code cookie bootstrap (§14.5): mint OAuth tokens from a claude.ai
//! `sessionKey` cookie. Ported from v1 `utils/claudecode_cookie.rs`, adapted to
//! the [`UpstreamClient`] transport (no `wreq`, no metadata tracking).
//!
//! Flow: cookie → `/api/bootstrap` org discovery → `/v1/oauth/{org}/authorize`
//! (PKCE) → `/v1/oauth/token` exchange. The client_id / scope / redirect_uri
//! MUST match the main channel's authcode flow — Anthropic validates the triple
//! at the authorize step. The minted secret is
//! `{access_token, refresh_token?, expires_at_ms, cookie}`; the cookie is kept
//! so a later operator can re-bootstrap, and so the channel can fall back to it.

use std::sync::Arc;

use bytes::Bytes;
use http::header::{ACCEPT, CONTENT_TYPE};
use http::{Request, Response};
use serde_json::Value;

use super::auth::{DEFAULT_REDIRECT_URI, OAUTH_CLIENT_ID, OAUTH_SCOPE};
use crate::channel::ChannelError;
use crate::channel::oauth;
use crate::http::client::UpstreamClient;

const CLAUDE_AI_BASE: &str = "https://claude.ai";
const API_BASE: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const USER_AGENT: &str = "claude-cli/2.1.162 (external, cli)";

/// Org capabilities that gate Claude Code OAuth (`user:inference` scope).
/// API-only orgs return `permission_error` at the authorize step, so the
/// subscription-capable membership is selected.
const SUBSCRIPTION_CAPS: &[&str] = &[
    "claude_pro",
    "claude_max",
    "claude_team",
    "claude_enterprise",
];

/// Bootstrap an OAuth secret from a claude.ai session cookie. See module docs
/// for the flow. The plaintext secret is returned for the caller to seal.
pub(super) async fn exchange(
    client: &Arc<dyn UpstreamClient>,
    cookie: &str,
) -> Result<Value, ChannelError> {
    let cookie = cookie.trim();
    if cookie.is_empty() {
        return Err(ChannelError::InvalidCredential("empty cookie".into()));
    }

    let org_uuid = discover_org(client, cookie).await?;
    let (verifier, challenge) = oauth::pkce();
    let state = crate::util::rand::uuid_v4();
    let code = authorize(client, cookie, &org_uuid, &state, &challenge).await?;
    let secret = token_exchange(client, &verifier, &state, &code).await?;

    let mut secret = secret;
    if let Some(obj) = secret.as_object_mut() {
        obj.insert("cookie".into(), Value::String(cookie.to_string()));
        obj.insert("account_uuid".into(), Value::String(org_uuid));
    }
    super::auth::ensure_device_id(&mut secret);
    Ok(secret)
}

/// Re-mint a secret from the stored `cookie` (§14.5 M7b): the cookie-only
/// refresh path for a credential that carries no `refresh_token`. Builds its own
/// Chrome-emulating client — claude.ai is Cloudflare-fronted and rejects
/// non-browser TLS, same as the login endpoint — and overlays the freshly minted
/// token/cookie/account fields onto the existing secret so operator fields the
/// bootstrap never sets (device_id / user_email …) survive the refresh.
///
/// Browser TLS is native-only; non-`upstream-wreq` builds cannot satisfy
/// Cloudflare and report [`ChannelError::Unsupported`] rather than fail opaquely.
pub(super) async fn refresh(secret: &Value) -> Result<Value, ChannelError> {
    let cookie = secret
        .get("cookie")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChannelError::InvalidCredential("missing cookie".into()))?;
    #[cfg(feature = "upstream-wreq")]
    {
        let client: Arc<dyn UpstreamClient> = Arc::new(
            crate::http::client::WreqClient::browser()
                .map_err(|e| ChannelError::Build(format!("cookie client init: {e}")))?,
        );
        let minted = exchange(&client, cookie).await?;
        Ok(overlay(secret, &minted))
    }
    #[cfg(not(feature = "upstream-wreq"))]
    {
        let _ = cookie;
        Err(ChannelError::Unsupported(
            "cookie refresh requires the browser-TLS (upstream-wreq) build",
        ))
    }
}

/// Overlay the freshly minted bootstrap secret onto the existing one: minted
/// token/cookie/account fields win; any other field already present (device_id /
/// user_email …) is preserved.
fn overlay(old: &Value, minted: &Value) -> Value {
    let mut out = old.clone();
    if let (Some(obj), Some(m)) = (out.as_object_mut(), minted.as_object()) {
        for (k, v) in m {
            obj.insert(k.clone(), v.clone());
        }
    }
    out
}

/// Step 1: GET `/api/bootstrap`, pick the first subscription-capable org uuid.
async fn discover_org(
    client: &Arc<dyn UpstreamClient>,
    cookie: &str,
) -> Result<String, ChannelError> {
    let req = cookie_get(format!("{CLAUDE_AI_BASE}/api/bootstrap"), cookie)?;
    let body = send_ok(client, req, "bootstrap").await?;
    // claude.ai may prepend a usage object before the bootstrap payload; scan
    // the JSON value stream for the one carrying `account`.
    let value = parse_bootstrap(&body)?;
    let org = value
        .get("account")
        .and_then(|a| a.get("memberships"))
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .filter_map(|m| m.get("organization"))
                .find(|o| org_has_subscription(o))
        })
        .and_then(|o| o.get("uuid"))
        .and_then(Value::as_str)
        .map(str::to_string);
    org.ok_or_else(|| {
        ChannelError::Build(
            "cookie auth: no subscription-capable organization (claude_pro/max/team/enterprise)"
                .into(),
        )
    })
}

/// Step 2: POST `/v1/oauth/{org}/authorize` with PKCE, extract `code` from the
/// returned `redirect_uri`.
async fn authorize(
    client: &Arc<dyn UpstreamClient>,
    cookie: &str,
    org_uuid: &str,
    state: &str,
    challenge: &str,
) -> Result<String, ChannelError> {
    let payload = serde_json::json!({
        "response_type": "code",
        "client_id": OAUTH_CLIENT_ID,
        "organization_uuid": org_uuid,
        "redirect_uri": DEFAULT_REDIRECT_URI,
        "scope": OAUTH_SCOPE,
        "state": state,
        "code_challenge": challenge,
        "code_challenge_method": "S256",
    });
    let body = serde_json::to_vec(&payload)
        .map_err(|e| ChannelError::Build(format!("authorize payload: {e}")))?;
    let req = Request::post(format!("{API_BASE}/v1/oauth/{org_uuid}/authorize"))
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .header("cookie", format!("sessionKey={cookie}"))
        .header("origin", CLAUDE_AI_BASE)
        .header("anthropic-version", API_VERSION)
        .header("anthropic-beta", OAUTH_BETA)
        .header(http::header::USER_AGENT, USER_AGENT)
        .body(Bytes::from(body))
        .map_err(|e| ChannelError::Build(format!("authorize request build: {e}")))?;
    let resp = send_ok(client, req, "authorize").await?;
    let value: Value = serde_json::from_slice(&resp)
        .map_err(|e| ChannelError::Build(format!("authorize response parse: {e}")))?;
    let redirect = value
        .get("redirect_uri")
        .and_then(Value::as_str)
        .ok_or_else(|| ChannelError::Build("authorize: missing redirect_uri".into()))?;
    query_param(redirect, "code")
        .ok_or_else(|| ChannelError::Build("authorize: missing code in redirect_uri".into()))
}

/// Step 3: POST `/v1/oauth/token` with the code + verifier + state, map to the
/// `{access_token, refresh_token?, expires_at_ms}` secret.
async fn token_exchange(
    client: &Arc<dyn UpstreamClient>,
    verifier: &str,
    state: &str,
    code: &str,
) -> Result<Value, ChannelError> {
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", OAUTH_CLIENT_ID),
        ("code", code),
        ("redirect_uri", DEFAULT_REDIRECT_URI),
        ("code_verifier", verifier),
        ("state", state),
    ];
    let extra = [
        ("anthropic-version", API_VERSION),
        ("anthropic-beta", OAUTH_BETA),
        ("user-agent", USER_AGENT),
        ("origin", CLAUDE_AI_BASE),
    ];
    let resp =
        oauth::token_post(client, &format!("{API_BASE}/v1/oauth/token"), &form, &extra).await?;
    let access_token = resp
        .access_token
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChannelError::Build("cookie token response missing access_token".into()))?;
    let expires_at_ms = crate::util::time::unix_now().saturating_mul(1000)
        + resp.expires_in.unwrap_or(3600) as i64 * 1000;
    let mut secret = serde_json::json!({
        "access_token": access_token,
        "expires_at_ms": expires_at_ms,
    });
    if let Some(rt) = resp.refresh_token.filter(|s| !s.is_empty()) {
        secret["refresh_token"] = Value::String(rt);
    }
    Ok(secret)
}

fn org_has_subscription(org: &Value) -> bool {
    org.get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|caps| {
            caps.iter()
                .filter_map(Value::as_str)
                .any(|s| SUBSCRIPTION_CAPS.contains(&s))
        })
}

/// Prefer the JSON-stream value carrying `account`; else the first value.
fn parse_bootstrap(body: &[u8]) -> Result<Value, ChannelError> {
    let stream = serde_json::Deserializer::from_slice(body).into_iter::<Value>();
    let mut first = None;
    for value in stream.flatten() {
        if value.get("account").and_then(Value::as_object).is_some() {
            return Ok(value);
        }
        if first.is_none() {
            first = Some(value);
        }
    }
    first.ok_or_else(|| ChannelError::Build("bootstrap: empty response".into()))
}

fn cookie_get(url: String, cookie: &str) -> Result<Request<Bytes>, ChannelError> {
    Request::get(url)
        .header(ACCEPT, "application/json")
        .header("cookie", format!("sessionKey={cookie}"))
        .header("origin", CLAUDE_AI_BASE)
        .body(Bytes::new())
        .map_err(|e| ChannelError::Build(format!("cookie request build: {e}")))
}

/// Send `req`, return the 2xx body. Non-2xx → `Build` with status + a snippet
/// (the cookie rides the header, never the logged form).
async fn send_ok(
    client: &Arc<dyn UpstreamClient>,
    req: Request<Bytes>,
    what: &str,
) -> Result<Bytes, ChannelError> {
    let resp: Response<Bytes> = client
        .send(req)
        .await
        .map_err(|e| ChannelError::Build(format!("{what} request failed: {e}")))?;
    let (parts, body) = resp.into_parts();
    if !parts.status.is_success() {
        let snippet: String = String::from_utf8_lossy(&body).chars().take(256).collect();
        return Err(ChannelError::Build(format!(
            "{what} endpoint {}: {snippet}",
            parts.status
        )));
    }
    Ok(body)
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overlay_refreshes_tokens_and_preserves_operator_fields() {
        // Cookie-only secret pre-refresh, carrying an operator field the
        // bootstrap never sets.
        let old = json!({
            "access_token": "stale",
            "cookie": "sessionKey-abc",
            "device_id": "dev-1",
        });
        // What the bootstrap mints (fresh tokens + account, same cookie).
        let minted = json!({
            "access_token": "fresh",
            "refresh_token": "rt-new",
            "expires_at_ms": 42,
            "cookie": "sessionKey-abc",
            "account_uuid": "org-9",
        });

        let out = overlay(&old, &minted);
        assert_eq!(out["access_token"], "fresh"); // minted wins
        assert_eq!(out["refresh_token"], "rt-new"); // minted adds it
        assert_eq!(out["account_uuid"], "org-9");
        assert_eq!(out["device_id"], "dev-1"); // operator field survives
    }
}
