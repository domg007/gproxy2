//! Google AI Studio (Gemini) channel — api key in the `?key=` query param,
//! default `https://generativelanguage.googleapis.com`.

mod auth;

use crate::channel::bulletins::common::{self, ApiKeyDefaults};
use crate::channel::http_util::{allow_headers, allow_query, build_request, join_url};
use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

const DEFAULTS: ApiKeyDefaults = ApiKeyDefaults {
    default_base_url: Some("https://generativelanguage.googleapis.com"),
    forward_headers: &[],
    forward_query: &[],
};

pub struct AiStudioChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for AiStudioChannel {
    fn id(&self) -> &'static str {
        "aistudio"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::GeminiGenerateContent
    }

    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        let base_url = common::resolve_base_url(&ctx, &DEFAULTS)?;
        let api_key = common::resolve_api_key(&ctx)?;
        let query = auth::apply_query(allow_query(ctx.query, DEFAULTS.forward_query), &api_key);
        let uri = join_url(&base_url, ctx.path, query.as_deref())?;
        let headers = allow_headers(ctx.headers, DEFAULTS.forward_headers);
        let req = build_request(ctx.method, uri, headers, ctx.body)?;
        Ok(PreparedRequest::new(req))
    }
}
