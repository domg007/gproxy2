//! GitHub Copilot CLI channel (GitHub OAuth device-flow → Copilot token). STUB —
//! registered so `copilot_cli` resolves, but `prepare` errors until the OAuth
//! infra (M7) lands. Endpoint/wire to be confirmed when implemented. See [`auth`].

mod auth;

use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

pub struct CopilotCliChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for CopilotCliChannel {
    fn id(&self) -> &'static str {
        "copilot_cli"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::OpenAiChatCompletions
    }

    fn prepare(&self, _ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        Err(ChannelError::Unsupported(
            "copilot_cli channel: GitHub OAuth device-flow not implemented yet (M7)",
        ))
    }
}
