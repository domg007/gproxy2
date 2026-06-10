//! Copilot CLI auth: re-exchange a long-lived GitHub token for a short-lived
//! Copilot token, and build the OpenAI-chat upstream headers.
//!
//! Token exchange is a GET against `api.github.com/copilot_internal/v2/token`
//! (NOT a form POST), so it bypasses [`crate::channel::oauth::token_post`] and
//! issues `client.send` directly. There is no `refresh_token`: every refresh
//! re-exchanges from the GitHub token.
//!
//! Login (M10, out of scope here): GitHub device flow — `POST
//! github.com/login/device/code` with `client_id = Iv1.b507a08c87ecfe98` and
//! `scope = read:user`, show the `user_code`, poll
//! `github.com/login/oauth/access_token` until the user authorizes, then store
//! the returned `access_token` as `github_token`. See the TS sample
//! `services/github/{get-device-code,poll-access-token}.ts`.

use std::sync::Arc;

use bytes::Bytes;
use http::header::{CONTENT_TYPE, HeaderName, HeaderValue};
use http::{Request, Response};
use serde::Deserialize;
use serde_json::Value;

use crate::channel::ChannelError;
use crate::http::client::UpstreamClient;

/// Editor identity the GitHub/Copilot endpoints expect (mirrors the VS Code
/// Copilot Chat extension; from the TS sample's `lib/api-config.ts`).
const DEFAULT_VSCODE_VERSION: &str = "1.95.3";
const EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.43.0";
const USER_AGENT: &str = "GitHubCopilotChat/0.43.0";
const API_VERSION: &str = "2025-04-01";
const GITHUB_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";

/// `api.github.com/copilot_internal/v2/token` response. Tolerant of unknown
/// fields; `expires_at` is unix SECONDS, `refresh_in` is unused here (we key
/// off `expires_at` with a skew in [`super`]).
#[derive(Debug, Deserialize)]
pub(super) struct CopilotTokenResponse {
    pub token: String,
    pub expires_at: i64,
}

/// Read the required long-lived GitHub token from the secret.
pub(super) fn github_token(secret: &Value) -> Result<&str, ChannelError> {
    secret
        .get("github_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChannelError::InvalidCredential("missing github_token".into()))
}

/// Emulated VS Code version (secret override, else the baked default).
pub(super) fn vscode_version(secret: &Value) -> &str {
    secret
        .get("vscode_version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_VSCODE_VERSION)
}

/// Copilot API base URL by `account_type` (default `individual`).
pub(super) fn base_url(secret: &Value) -> String {
    match secret.get("account_type").and_then(Value::as_str) {
        Some("business") => "https://api.business.githubcopilot.com".into(),
        Some("enterprise") => "https://api.enterprise.githubcopilot.com".into(),
        _ => "https://api.githubcopilot.com".into(),
    }
}

/// GET the Copilot token from GitHub using the long-lived `github_token`. The
/// caller persists the result; this only performs the exchange. Non-2xx →
/// [`ChannelError::Build`] with the status + a truncated body snippet.
pub(super) async fn exchange_copilot_token(
    client: &Arc<dyn UpstreamClient>,
    github_token: &str,
    vscode_version: &str,
) -> Result<CopilotTokenResponse, ChannelError> {
    let req = Request::get(GITHUB_TOKEN_URL)
        .header("authorization", format!("token {github_token}"))
        .header("editor-version", format!("vscode/{vscode_version}"))
        .header("editor-plugin-version", EDITOR_PLUGIN_VERSION)
        .header("user-agent", USER_AGENT)
        .header("x-github-api-version", API_VERSION)
        .header(http::header::ACCEPT, "application/json")
        .body(Bytes::new())
        .map_err(|e| ChannelError::Build(format!("copilot token request build: {e}")))?;

    let resp: Response<Bytes> = client
        .send(req)
        .await
        .map_err(|e| ChannelError::Build(format!("copilot token request failed: {e}")))?;
    let (parts, body) = resp.into_parts();
    if !parts.status.is_success() {
        let snippet: String = String::from_utf8_lossy(&body).chars().take(256).collect();
        return Err(ChannelError::Build(format!(
            "copilot token endpoint {}: {snippet}",
            parts.status
        )));
    }
    serde_json::from_slice(&body)
        .map_err(|e| ChannelError::Build(format!("copilot token response parse: {e}")))
}

/// Inject the Copilot OpenAI-chat headers onto the prepared upstream request.
/// `body` is the already-assembled upstream body, inspected only to pick
/// `X-Initiator` (agent when assistant/tool turns are present, else user).
pub(super) fn apply_chat_headers(
    req: &mut Request<Bytes>,
    copilot_token: &str,
    vscode_version: &str,
) -> Result<(), ChannelError> {
    let bearer = HeaderValue::from_str(&format!("Bearer {copilot_token}"))
        .map_err(|e| ChannelError::InvalidCredential(format!("bad copilot_token: {e}")))?;
    let editor = HeaderValue::from_str(&format!("vscode/{vscode_version}"))
        .map_err(|e| ChannelError::Build(format!("bad editor-version: {e}")))?;
    let request_id = HeaderValue::from_str(&new_request_id())
        .map_err(|e| ChannelError::Build(format!("bad x-request-id: {e}")))?;
    let initiator = if has_assistant_or_tool_turn(req.body()) {
        "agent"
    } else {
        "user"
    };

    let h = req.headers_mut();
    h.insert(http::header::AUTHORIZATION, bearer);
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h.insert(
        HeaderName::from_static("copilot-integration-id"),
        HeaderValue::from_static("vscode-chat"),
    );
    h.insert(HeaderName::from_static("editor-version"), editor);
    h.insert(
        HeaderName::from_static("editor-plugin-version"),
        HeaderValue::from_static(EDITOR_PLUGIN_VERSION),
    );
    h.insert(
        http::header::USER_AGENT,
        HeaderValue::from_static(USER_AGENT),
    );
    h.insert(
        HeaderName::from_static("openai-intent"),
        HeaderValue::from_static("conversation-panel"),
    );
    h.insert(
        HeaderName::from_static("x-github-api-version"),
        HeaderValue::from_static(API_VERSION),
    );
    h.insert(HeaderName::from_static("x-request-id"), request_id);
    h.insert(
        HeaderName::from_static("x-initiator"),
        HeaderValue::from_static(initiator),
    );
    Ok(())
}

/// `X-Initiator` heuristic: Copilot expects `agent` once the conversation
/// contains an assistant or tool turn, else `user`. A cheap substring probe on
/// the JSON body is enough — a false `user` is harmless, and we never mutate
/// the body. Falls back to `user` on non-JSON / parse failure.
fn has_assistant_or_tool_turn(body: &Bytes) -> bool {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    v.get("messages")
        .and_then(Value::as_array)
        .is_some_and(|msgs| {
            msgs.iter().any(|m| {
                matches!(
                    m.get("role").and_then(Value::as_str),
                    Some("assistant") | Some("tool")
                )
            })
        })
}

/// Fresh per-request id for `x-request-id`. `uuid` is a native-only dependency
/// (see Cargo.toml target tables), so wasm falls back to a JS-clock + counter
/// id — same split as [`crate::http::server::extract`].
#[cfg(not(target_arch = "wasm32"))]
fn new_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(target_arch = "wasm32")]
fn new_request_id() -> String {
    use core::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{:x}", js_sys::Date::now() as u64, n)
}
