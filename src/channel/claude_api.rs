//! Anthropic Claude channel (API-key auth, `x-api-key` + `anthropic-version`).

use http::HeaderName;
use http::header::HeaderValue;

use crate::channel::http_util::{build_request, join_url, sanitize_headers};
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

        let uri = join_url(base_url, ctx.path, ctx.query)?;
        let headers = sanitize_headers(ctx.headers);
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
