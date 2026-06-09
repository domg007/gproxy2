//! Groq channel — `Authorization: Bearer`, default `https://api.groq.com/openai`.

mod auth;

use crate::channel::bulletins::common::{self, ApiKeyDefaults};
use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

const DEFAULTS: ApiKeyDefaults = ApiKeyDefaults {
    default_base_url: Some("https://api.groq.com/openai"),
    forward_headers: &[],
    forward_query: &[],
};

pub struct GroqChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for GroqChannel {
    fn id(&self) -> &'static str {
        "groq"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::OpenAiChatCompletions
    }

    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        let (mut req, key) = common::build_request(ctx, &DEFAULTS)?;
        auth::apply(&mut req, &key)?;
        Ok(PreparedRequest {
            request: req,
            proxy_url: None,
        })
    }
}
