//! Generic OpenAI-compatible channel (API-key auth, `Authorization: Bearer`).

use http::header::{AUTHORIZATION, HeaderValue};

use crate::channel::http_util::{allow_headers, allow_query, build_request, join_url};
use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

/// OpenAI-compatible upstream: `base_url` + `Authorization: Bearer <api_key>`.
pub struct OpenAiCompatChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for OpenAiCompatChannel {
    fn id(&self) -> &'static str {
        "openai_compatible"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::OpenAiChatCompletions
    }

    fn forward_headers(&self) -> &'static [&'static str] {
        &["openai-beta", "openai-organization", "openai-project"]
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

        let auth = HeaderValue::from_str(&format!("Bearer {api_key}"))
            .map_err(|e| ChannelError::InvalidCredential(format!("bad api_key: {e}")))?;
        request.headers_mut().insert(AUTHORIZATION, auth);

        Ok(PreparedRequest {
            request,
            proxy_url: None,
        })
    }
}
