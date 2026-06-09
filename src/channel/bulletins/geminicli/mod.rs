//! Gemini CLI channel (Google Code Assist, OAuth2 + PKCE; code-assist envelope).
//! STUB — registered so `geminicli` resolves, but `prepare` errors until the
//! OAuth refresh infra (M7) and the envelope transform (M2) land. See [`auth`].

mod auth;

use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

pub struct GeminiCliChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for GeminiCliChannel {
    fn id(&self) -> &'static str {
        "geminicli"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::GeminiGenerateContent
    }

    fn prepare(&self, _ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        Err(ChannelError::Unsupported(
            "geminicli channel: OAuth + code-assist envelope not implemented yet (M7/M2)",
        ))
    }
}
