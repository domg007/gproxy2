//! Anthropic Claude channel — `x-api-key` + `anthropic-version`, default
//! `https://api.anthropic.com`.

mod auth;

use crate::channel::bulletins::common::{self, ApiKeyDefaults};
use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::Provider;

const DEFAULTS: ApiKeyDefaults = ApiKeyDefaults {
    default_base_url: Some("https://api.anthropic.com"),
    forward_headers: &["anthropic-beta"],
    forward_query: &[],
};

pub struct ClaudeApiChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for ClaudeApiChannel {
    fn id(&self) -> &'static str {
        "claude_api"
    }

    fn provider_family(&self) -> Provider {
        Provider::Claude
    }

    fn routing_table(&self) -> crate::channel::routes::RouteList {
        use crate::channel::routes::{cg, pass, pv, xform};
        use crate::protocol::{ContentGenerationKind::*, Operation::*, Provider as P};
        vec![
            pass(ListModels, pv(P::Claude)),
            pass(ListModels, pv(P::OpenAi)),
            xform(ListModels, pv(P::Gemini), ListModels, pv(P::Claude)),
            pass(GetModel, pv(P::Claude)),
            pass(GetModel, pv(P::OpenAi)),
            xform(GetModel, pv(P::Gemini), GetModel, pv(P::Claude)),
            pass(CountTokens, pv(P::Claude)),
            xform(CountTokens, pv(P::OpenAi), CountTokens, pv(P::Claude)),
            xform(CountTokens, pv(P::Gemini), CountTokens, pv(P::Claude)),
            pass(GenerateContent, cg(ClaudeMessages)),
            pass(GenerateContent, cg(OpenAiChatCompletions)),
            xform(
                GenerateContent,
                cg(OpenAiResponses),
                GenerateContent,
                cg(ClaudeMessages),
            ),
            xform(
                GenerateContent,
                cg(GeminiGenerateContent),
                GenerateContent,
                cg(ClaudeMessages),
            ),
            pass(StreamGenerateContent, cg(ClaudeMessages)),
            pass(StreamGenerateContent, cg(OpenAiChatCompletions)),
            xform(
                StreamGenerateContent,
                cg(OpenAiResponses),
                StreamGenerateContent,
                cg(ClaudeMessages),
            ),
            xform(
                StreamGenerateContent,
                cg(GeminiGenerateContent),
                StreamGenerateContent,
                cg(ClaudeMessages),
            ),
            xform(
                CompactContent,
                pv(P::OpenAi),
                GenerateContent,
                cg(ClaudeMessages),
            ),
        ]
    }

    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        let (mut req, key) = common::build_request(ctx, &DEFAULTS)?;
        auth::apply(&mut req, &key)?;
        Ok(PreparedRequest::new(req))
    }
}
