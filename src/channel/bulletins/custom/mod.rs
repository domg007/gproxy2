//! Custom (universal) channel — a generic passthrough to any OpenAI / Claude /
//! Gemini-compatible endpoint. `base_url` is REQUIRED (no baked default); the
//! auth header is chosen by the inbound protocol (see [`auth`]).

mod auth;

use crate::channel::bulletins::common::{self, ApiKeyDefaults};
use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

const DEFAULTS: ApiKeyDefaults = ApiKeyDefaults {
    default_base_url: None, // base_url must be supplied in settings_json
    forward_headers: &[],
    forward_query: &[],
};

pub struct CustomChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for CustomChannel {
    fn id(&self) -> &'static str {
        "custom"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        // Universal passthrough; the inbound kind governs. Default label only.
        ContentGenerationKind::OpenAiChatCompletions
    }

    fn prepare(&self, ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        // Decide the auth style from the inbound path BEFORE `ctx` is consumed.
        let proto = auth::detect(ctx.path);
        let (mut req, key) = common::build_request(ctx, &DEFAULTS)?;
        auth::apply(&mut req, &key, proto)?;
        Ok(PreparedRequest {
            request: req,
            proxy_url: None,
        })
    }
}
