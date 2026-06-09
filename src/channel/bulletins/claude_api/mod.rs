//! Anthropic Claude channel — `x-api-key` + `anthropic-version`, default
//! `https://api.anthropic.com`.

mod auth;

use crate::channel::bulletins::common::{self, ApiKeyDefaults};
use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

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

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::ClaudeMessages
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
