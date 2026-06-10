//! Claude Code channel — Anthropic Messages API over OAuth2 (`refresh_token`
//! grant) plus the claude-cli / `@anthropic-ai/sdk` impersonation header set.
//!
//! `target_kind` is `ClaudeMessages`: the request is a verbatim Claude-messages
//! passthrough — NO envelope, NO stream decoder, NO normalize. [`auth`] owns the
//! OAuth refresh + header injection (and documents the deferred cookie login).

mod auth;

use std::sync::Arc;

use serde_json::Value;

use crate::channel::http_util::{allow_headers, build_request, join_url};
use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
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

    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        let access_token = auth::access_token(ctx.secret)?.to_string();
        let base = ctx
            .provider_settings
            .get("base_url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(auth::DEFAULT_BASE_URL);

        // Claude-messages passthrough: the inbound path is already provider
        // relative (`/v1/messages`, `/v1/messages/count_tokens`, …); forward it.
        let uri = join_url(base, ctx.path, ctx.query)?;
        // Impersonation channel: it injects its own fingerprint headers and only
        // forwards `anthropic-beta` from the client (base allow-list adds
        // content-type / accept).
        let headers = allow_headers(ctx.headers, &["anthropic-beta"]);
        let mut req = build_request(ctx.method, uri, headers, ctx.body)?;
        auth::apply(&mut req, &access_token)?;
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
            "claude-cli/2.1.154 (external, cli)"
        );
        assert!(req.headers().get("x-claude-code-session-id").is_some());
    }
}
