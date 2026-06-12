//! Claude Code channel — Anthropic Messages API over OAuth2 (`refresh_token`
//! grant) plus the claude-cli / `@anthropic-ai/sdk` impersonation header set.
//!
//! `target_kind` is `ClaudeMessages`: the request is a verbatim Claude-messages
//! passthrough — NO envelope, NO stream decoder, NO normalize. [`auth`] owns the
//! OAuth refresh + header injection (with a cookie-bootstrap refresh fallback for
//! credentials that carry only a `cookie`).

mod auth;
mod cch;
mod cookie;
#[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
mod fingerprint;

use std::sync::Arc;

use bytes::Bytes;
use serde_json::Value;

use crate::channel::http_util::{allow_headers, build_request, join_url};
use crate::channel::{
    AuthCodeStart, Channel, ChannelError, ChannelLogin, PrepareCtx, PreparedRequest,
};
use crate::http::client::UpstreamClient;
use crate::protocol::ContentGenerationKind;

pub struct ClaudeCodeChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for ClaudeCodeChannel {
    fn id(&self) -> &'static str {
        "claudecode"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::ClaudeMessages
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
    fn default_emulation(&self) -> Option<wreq::Emulation> {
        Some(fingerprint::default_emulation())
    }

    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        let access_token = auth::access_token(ctx.secret)?.to_string();
        let base = ctx
            .provider_settings
            .get("base_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(auth::DEFAULT_BASE_URL);

        // Stable per-credential `device_id`; `session_id` is deterministic per
        // (device, conversation, hour) and capped at ≤1000 slots — the SAME value
        // is sent as `x-claude-code-session-id` and inside `metadata.user_id`.
        let device_id = auth::device_id(ctx.secret);
        let now_secs = crate::util::time::unix_now().max(0) as u64;
        let session_id = cch::session_id(&device_id, &ctx.body, now_secs);

        // The model call (`POST /v1/messages*`) carries the CLI billing header +
        // `metadata.user_id`; the `cch` checksum is computed over the final body.
        // Other paths (e.g. token-count GET) pass through unchanged.
        let is_messages = ctx.method == http::Method::POST && ctx.path.starts_with("/v1/messages");
        let body = if is_messages {
            let account_uuid = ctx
                .secret
                .get("account_uuid")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Bytes::from(cch::apply(
                &ctx.body,
                &device_id,
                account_uuid,
                &session_id,
                "cli",
            ))
        } else {
            ctx.body
        };

        // Claude-messages passthrough: the inbound path is already provider
        // relative (`/v1/messages`, `/v1/messages/count_tokens`, …); forward it.
        let uri = join_url(base, ctx.path, ctx.query)?;
        // Impersonation channel: it injects its own fingerprint headers and only
        // forwards `anthropic-beta` from the client (base allow-list adds
        // content-type / accept).
        let headers = allow_headers(ctx.headers, &["anthropic-beta"]);
        let mut req = build_request(ctx.method, uri, headers, body)?;
        // `x-client-request-id` rides only the direct api.anthropic.com model call.
        let with_request_id = is_messages && base == auth::DEFAULT_BASE_URL;
        auth::apply(&mut req, &access_token, &session_id, with_request_id)?;
        Ok(PreparedRequest::new(req))
    }

    fn needs_refresh(&self, secret: &Value) -> bool {
        auth::needs_refresh(secret)
    }

    async fn refresh(
        &self,
        client: &Arc<dyn UpstreamClient>,
        secret: &Value,
    ) -> Result<Value, ChannelError> {
        auth::refresh(client, secret).await
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl ChannelLogin for ClaudeCodeChannel {
    fn authcode_start(
        &self,
        redirect_uri: &str,
        state: &str,
        pkce_challenge: &str,
    ) -> Option<AuthCodeStart> {
        let (authorize_url, redirect_uri) =
            auth::authcode_start(redirect_uri, state, pkce_challenge);
        Some(AuthCodeStart {
            authorize_url,
            redirect_uri,
        })
    }

    async fn authcode_exchange(
        &self,
        client: &Arc<dyn UpstreamClient>,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<Value, ChannelError> {
        auth::authcode_exchange(client, code, verifier, redirect_uri).await
    }

    async fn cookie_exchange(
        &self,
        client: &Arc<dyn UpstreamClient>,
        cookie: &str,
    ) -> Result<Value, ChannelError> {
        cookie::exchange(client, cookie).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::{HeaderMap, Method, Response};
    use serde_json::json;

    use crate::http::client::ClientError;

    /// Canned token-endpoint mock: returns a rotated access/refresh pair so
    /// `refresh` can be exercised without the pipeline.
    struct MockUpstream;
    #[async_trait::async_trait]
    impl UpstreamClient for MockUpstream {
        async fn send(&self, _req: http::Request<Bytes>) -> Result<Response<Bytes>, ClientError> {
            Ok(Response::builder()
                .status(200)
                .body(Bytes::from_static(
                    br#"{"access_token":"new","refresh_token":"newrt","expires_in":3600}"#,
                ))
                .unwrap())
        }
    }

    #[tokio::test]
    async fn refresh_rotates_tokens() {
        let secret = json!({
            "access_token": "old",
            "refresh_token": "oldrt",
            "expires_at_ms": 1,
            "account_uuid": "acct-123",
        });
        let client: Arc<dyn UpstreamClient> = Arc::new(MockUpstream);
        let out = ClaudeCodeChannel.refresh(&client, &secret).await.unwrap();

        assert_eq!(out["access_token"], "new");
        assert_eq!(out["refresh_token"], "newrt"); // rotated, not the old one
        assert!(out["expires_at_ms"].as_i64().unwrap() > 0);
        // Unrelated fields preserved.
        assert_eq!(out["account_uuid"], "acct-123");
    }

    #[test]
    fn prepare_injects_oauth_and_stainless() {
        let secret = json!({ "access_token": "tok-abc" });
        let settings = json!({});
        let headers = HeaderMap::new();
        let ctx = PrepareCtx {
            secret: &secret,
            provider_settings: &settings,
            upstream_model_id: "claude-sonnet-4",
            method: Method::POST,
            path: "/v1/messages",
            query: None,
            headers: &headers,
            body: Bytes::from_static(b"{\"model\":\"claude-sonnet-4\"}"),
        };
        let req = ClaudeCodeChannel.prepare(ctx).unwrap().request;

        assert_eq!(
            req.uri().to_string(),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            req.headers().get("authorization").unwrap(),
            "Bearer tok-abc"
        );
        assert_eq!(
            req.headers().get("anthropic-beta").unwrap(),
            "oauth-2025-04-20"
        );
        assert_eq!(req.headers().get("x-app").unwrap(), "cli");
        assert_eq!(req.headers().get("x-stainless-lang").unwrap(), "js");
        assert_eq!(req.headers().get("x-stainless-runtime").unwrap(), "node");
        assert_eq!(
            req.headers().get("user-agent").unwrap(),
            "claude-cli/2.1.162 (external, cli)"
        );
        assert!(req.headers().get("x-claude-code-session-id").is_some());
    }

    #[test]
    fn anthropic_beta_oauth_first_then_client_deduped() {
        let secret = json!({ "access_token": "tok" });
        let settings = json!({});
        let mut headers = HeaderMap::new();
        headers.insert(
            "anthropic-beta",
            "feat-x,oauth-2025-04-20,feat-y".parse().unwrap(),
        );
        let ctx = PrepareCtx {
            secret: &secret,
            provider_settings: &settings,
            upstream_model_id: "claude-sonnet-4",
            method: Method::POST,
            path: "/v1/messages",
            query: None,
            headers: &headers,
            body: Bytes::from_static(b"{\"messages\":[]}"),
        };
        let req = ClaudeCodeChannel.prepare(ctx).unwrap().request;
        // oauth marker first, client betas after, client's own oauth not duped.
        assert_eq!(
            req.headers().get("anthropic-beta").unwrap(),
            "oauth-2025-04-20,feat-x,feat-y"
        );
    }

    #[test]
    fn authcode_start_urls() {
        // claudecode + geminicli authcode_start build provider authorize URLs
        // carrying their client_id, the PKCE challenge, state, S256, a default
        // redirect_uri, and their scopes.
        let cc = ClaudeCodeChannel
            .authcode_start("", "ST", "CH")
            .expect("claudecode supports authcode");
        let url = &cc.authorize_url;
        assert!(
            url.starts_with("https://claude.ai/oauth/authorize?"),
            "{url}"
        );
        assert!(
            url.contains("client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e"),
            "{url}"
        );
        assert!(url.contains("code_challenge=CH"), "{url}");
        assert!(url.contains("state=ST"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        assert!(url.contains("redirect_uri="), "{url}");
        assert!(url.contains("scope=user%3Aprofile"), "{url}");
        assert_eq!(cc.redirect_uri, super::auth::DEFAULT_REDIRECT_URI);

        let gc = crate::channel::bulletins::geminicli::GeminiCliChannel
            .authcode_start("", "ST", "CH")
            .expect("geminicli supports authcode");
        let url = &gc.authorize_url;
        assert!(
            url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"),
            "{url}"
        );
        assert!(url.contains("681255809395-"), "{url}");
        assert!(url.contains("code_challenge=CH"), "{url}");
        assert!(url.contains("state=ST"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        assert!(url.contains("redirect_uri="), "{url}");
        assert!(url.contains("cloud-platform"), "{url}");
    }
}
