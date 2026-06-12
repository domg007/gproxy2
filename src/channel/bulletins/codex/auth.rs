//! Codex auth — OpenAI ChatGPT-backend OAuth2 `refresh_token` grant against
//! `auth.openai.com`; base `https://chatgpt.com/backend-api/codex`. Request
//! bodies are normalized to the private Responses API (see
//! [`normalize_responses_body`]); the inbound `/v1/responses` path is rewritten
//! to `/responses` by [`super::CodexChannel::prepare`].
//!
//! The login-time authorization-code + PKCE flow (client_id
//! `app_EMoamEEZ73f0CkXaXp7hrann`) is an M10 concern; this module covers only
//! the per-request access-token use and the refresh the pipeline drives. For
//! headless setups, the secret is provisioned via §14.5 remote-paste.
//!
//! As an impersonation channel it injects the codex-cli fingerprint / protocol
//! headers (its per-channel allow-list, applied after the global blacklist):
//! `user-agent`, `originator`, `session-id`, `thread-id`, `x-client-request-id`,
//! `x-codex-beta-features`, `x-codex-turn-metadata`, `x-codex-window-id`.
//! (`accept: text/event-stream` rides the base allow-list.)

use std::sync::Arc;

use bytes::Bytes;
use http::Request;
use http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderName, HeaderValue, USER_AGENT};
use serde_json::{Value, json};

use crate::channel::ChannelError;
use crate::channel::oauth;
use crate::http::client::UpstreamClient;

/// Public Codex CLI OAuth client (the credentials the official CLI ships with).
pub(super) const OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub(super) const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
/// Authorization endpoint for the interactive authcode+PKCE login (§14.5).
pub(super) const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";

/// Default redirect_uri + scopes the Codex CLI uses (mined from v1 codex.rs).
/// The CLI listens on `localhost:1455` and exchanges the code with this exact
/// value, so `complete` must echo it back.
pub(super) const DEFAULT_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub(super) const OAUTH_SCOPE: &str =
    "openid profile email offline_access api.connectors.read api.connectors.invoke";

/// ChatGPT codex-backend host. The Responses endpoint lives at `/responses`.
pub(super) const DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

/// User-Agent the Codex CLI (`codex_cli_rs`) sends; rides the credential's
/// `tls_fingerprint` pool (M7a). The `originator` header carries the same id.
const ORIGINATOR: &str = "codex_cli_rs";
const USER_AGENT_VALUE: &str = "codex_cli_rs/0.118.0 (Linux 6.6; x86_64)";

/// Refresh slightly before expiry to avoid racing a 401 mid-flight.
const EXPIRY_SKEW_MS: i64 = 60_000;

/// Keys stripped from the Responses body — the ChatGPT backend rejects or
/// ignores them (it pins sampling itself and never persists).
const STRIP_KEYS: &[&str] = &[
    "max_output_tokens",
    "metadata",
    "stream_options",
    "temperature",
    "top_p",
    "top_logprobs",
    "safety_identifier",
    "truncation",
];

/// Build the authorize URL for the interactive authcode+PKCE login. An empty
/// `redirect_uri` falls back to [`DEFAULT_REDIRECT_URI`]. Returns the URL plus
/// the effective redirect_uri (so `complete` exchanges with the same value).
///
/// The query carries the standard authcode+PKCE set plus the two codex-specific
/// flags the CLI sends (`id_token_add_organizations`, `codex_cli_simplified_flow`)
/// and `originator`, mined from v1 codex.rs.
pub(super) fn authcode_start(redirect_uri: &str, state: &str, challenge: &str) -> (String, String) {
    let redirect_uri = if redirect_uri.trim().is_empty() {
        DEFAULT_REDIRECT_URI
    } else {
        redirect_uri
    };
    let query = [
        ("response_type", "code"),
        ("client_id", OAUTH_CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", OAUTH_SCOPE),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
        ("originator", ORIGINATOR),
    ]
    .iter()
    .map(|(k, v)| format!("{k}={}", pct(v)))
    .collect::<Vec<_>>()
    .join("&");
    (format!("{AUTHORIZE_URL}?{query}"), redirect_uri.to_string())
}

/// Exchange an authorization code (+PKCE verifier) for the plaintext secret.
/// Maps the token response to `{access_token, refresh_token?, expires_at_ms}`.
/// (`id_token`/`account_id` are not surfaced by the shared `token_post`; the
/// pipeline refresh / first usage backfills `account_id` — see module docs.)
pub(super) async fn authcode_exchange(
    client: &Arc<dyn UpstreamClient>,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<Value, ChannelError> {
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", OAUTH_CLIENT_ID),
        ("code_verifier", verifier),
    ];
    let resp = oauth::token_post(client, TOKEN_URL, &form, &[]).await?;

    let access_token = resp
        .access_token
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChannelError::Build("token response missing access_token".into()))?;
    let expires_at_ms = crate::util::time::unix_now().saturating_mul(1000)
        + resp.expires_in.unwrap_or(3600) as i64 * 1000;

    let mut secret = json!({
        "access_token": access_token,
        "expires_at_ms": expires_at_ms,
    });
    if let Some(rt) = resp.refresh_token.filter(|s| !s.is_empty()) {
        secret["refresh_token"] = Value::String(rt);
    }
    Ok(secret)
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

/// Read a trimmed, non-empty string field from the secret.
fn secret_str<'a>(secret: &'a Value, key: &str) -> Option<&'a str> {
    secret
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// The OAuth access token, required by [`super::CodexChannel::prepare`].
pub(super) fn access_token(secret: &Value) -> Result<&str, ChannelError> {
    secret_str(secret, "access_token")
        .ok_or_else(|| ChannelError::InvalidCredential("missing access_token".into()))
}

/// The ChatGPT account id, sent as `chatgpt-account-id` when present.
pub(super) fn account_id(secret: &Value) -> Option<&str> {
    secret_str(secret, "account_id")
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

/// Refresh via the OpenAI `refresh_token` grant, returning the new plaintext
/// secret (`access_token` + `expires_at_ms` rotate; `refresh_token` and
/// `id_token` rotate when the response carries them, else the old ones are
/// kept; every other field — notably `account_id` — is preserved).
pub(super) async fn refresh(
    client: &Arc<dyn UpstreamClient>,
    secret: &Value,
) -> Result<Value, ChannelError> {
    let refresh_token = secret_str(secret, "refresh_token")
        .ok_or_else(|| ChannelError::InvalidCredential("missing refresh_token".into()))?;

    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", OAUTH_CLIENT_ID),
    ];
    let resp = oauth::token_post(client, TOKEN_URL, &form, &[]).await?;

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
    // refresh_token ROTATES when present — store the new one, else keep the old.
    if let Some(rt) = resp.refresh_token.filter(|s| !s.is_empty()) {
        obj.insert("refresh_token".into(), Value::String(rt));
    }
    // `account_id` / `id_token` are preserved by the clone (the shared
    // `token_post` does not surface a rotated `id_token`).
    obj.insert("expires_at_ms".into(), Value::Number(expires_at_ms.into()));
    Ok(out)
}

/// Inject the OAuth bearer + Codex CLI fingerprint headers onto the prepared
/// upstream request. A fresh per-request session id is generated and shared by
/// `session_id` and `x-client-request-id` (the backend expects them to match).
/// `accept` is `text/event-stream` since the body always forces `stream:true`.
pub(super) fn apply(
    req: &mut Request<Bytes>,
    access_token: &str,
    account_id: Option<&str>,
) -> Result<(), ChannelError> {
    let bearer = HeaderValue::from_str(&format!("Bearer {access_token}"))
        .map_err(|e| ChannelError::InvalidCredential(format!("bad access_token: {e}")))?;
    let session_id = HeaderValue::from_str(&new_session_id())
        .map_err(|e| ChannelError::Build(format!("bad session id: {e}")))?;

    let h = req.headers_mut();
    h.insert(AUTHORIZATION, bearer);
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    h.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    h.insert(
        HeaderName::from_static("originator"),
        HeaderValue::from_static(ORIGINATOR),
    );
    h.insert(HeaderName::from_static("session_id"), session_id.clone());
    h.insert(HeaderName::from_static("x-client-request-id"), session_id);
    if let Some(acct) = account_id {
        let acct = HeaderValue::from_str(acct)
            .map_err(|e| ChannelError::InvalidCredential(format!("bad account_id: {e}")))?;
        h.insert(HeaderName::from_static("chatgpt-account-id"), acct);
    }
    Ok(())
}

/// Normalize a Responses request body to what the ChatGPT codex backend expects.
///
/// - forces `stream:true`, `store:false`;
/// - drops the [`STRIP_KEYS`] (sampling / persistence / metadata fields);
/// - lifts instructions: a string `input` becomes a single user message; system
///   messages in an array `input` are pulled out, their text appended to the
///   top-level `instructions` (newline-joined), and removed from `input`.
///
/// Non-JSON bodies are forwarded verbatim (this never fails the request).
pub(super) fn normalize_responses_body(body: &Bytes) -> Bytes {
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return body.clone();
    };
    let Some(map) = value.as_object_mut() else {
        return body.clone();
    };

    map.insert("stream".into(), Value::Bool(true));
    map.insert("store".into(), Value::Bool(false));
    for key in STRIP_KEYS {
        map.remove(*key);
    }

    let mut instructions = map
        .get("instructions")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // A string `input` is the bare user prompt → wrap as a message item.
    if let Some(text) = map.get("input").and_then(Value::as_str) {
        let text = text.to_string();
        map.insert(
            "input".into(),
            json!([{ "type": "message", "role": "user", "content": text }]),
        );
    }

    // Lift `role:"system"` messages out of an array `input` into instructions.
    if let Some(Value::Array(items)) = map.get_mut("input") {
        let mut retained = Vec::with_capacity(items.len());
        for item in std::mem::take(items) {
            if is_system_role(&item) {
                append_instruction(&mut instructions, item_text(&item));
            } else {
                retained.push(item);
            }
        }
        *items = retained;
    }

    map.insert("instructions".into(), Value::String(instructions));
    serde_json::to_vec(&value)
        .map(Bytes::from)
        .unwrap_or_else(|_| body.clone())
}

/// Whether an `input` item is a `role:"system"` message (case-insensitive).
fn is_system_role(item: &Value) -> bool {
    item.get("role")
        .and_then(Value::as_str)
        .is_some_and(|r| r.eq_ignore_ascii_case("system"))
}

/// Append `text` to `instructions`, newline-joining onto any existing content.
fn append_instruction(instructions: &mut String, text: String) {
    if text.is_empty() {
        return;
    }
    if !instructions.is_empty() {
        instructions.push('\n');
    }
    instructions.push_str(&text);
}

/// Flatten a message item's `content` to text: a bare string, or the joined
/// `text` fields of an array of content parts.
fn item_text(item: &Value) -> String {
    let mut parts = Vec::new();
    collect_text(item.get("content").unwrap_or(&Value::Null), &mut parts);
    parts.join("\n")
}

fn collect_text(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::String(text) if !text.is_empty() => parts.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| collect_text(item, parts)),
        Value::Object(object) => {
            if let Some(text) = object.get("text").and_then(Value::as_str)
                && !text.is_empty()
            {
                parts.push(text.to_string());
            }
        }
        _ => {}
    }
}

/// Fresh per-request v4 session id (cross-target, cryptographically random).
fn new_session_id() -> String {
    crate::util::rand::uuid_v4()
}
