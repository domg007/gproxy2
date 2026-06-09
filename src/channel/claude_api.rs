//! Anthropic Claude channel (API-key auth, `x-api-key` + `anthropic-version`).

use http::HeaderName;
use http::header::HeaderValue;

use crate::channel::http_util::{allow_headers, allow_query, build_request, join_url};
use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Claude Messages upstream: `base_url` + `x-api-key` + `anthropic-version`.
pub struct ClaudeApiChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for ClaudeApiChannel {
    fn id(&self) -> &'static str {
        "claude_api"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::ClaudeMessages
    }

    fn forward_headers(&self) -> &'static [&'static str] {
        // anthropic-version is injected below (ours wins), so it is not forwarded.
        &["anthropic-beta"]
    }

    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        let base_url = ctx
            .provider_settings
            .get("base_url")
            .and_then(|v| v.as_str())
            .ok_or(ChannelError::MissingSetting("base_url"))?;
        let api_key = ctx
            .secret
            .get("api_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ChannelError::InvalidCredential("missing api_key".into()))?;

        let query = allow_query(ctx.query, self.forward_query());
        let uri = join_url(base_url, ctx.path, query.as_deref())?;
        let headers = allow_headers(ctx.headers, self.forward_headers());
        let mut request = build_request(ctx.method, uri, headers, ctx.body)?;

        let key = HeaderValue::from_str(api_key)
            .map_err(|e| ChannelError::InvalidCredential(format!("bad api_key: {e}")))?;
        let h = request.headers_mut();
        h.insert(HeaderName::from_static("x-api-key"), key);
        h.insert(
            HeaderName::from_static("anthropic-version"),
            HeaderValue::from_static(ANTHROPIC_VERSION),
        );

        Ok(PreparedRequest {
            request,
            proxy_url: None,
        })
    }
}
