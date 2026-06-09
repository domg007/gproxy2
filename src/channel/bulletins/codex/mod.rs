//! Codex channel (OpenAI ChatGPT backend, OAuth2 + PKCE; Responses/compact).
//! STUB — registered so `codex` resolves, but `prepare` errors until the OAuth
//! refresh infra (M7) and the Responses/compact handling (M2) land. See [`auth`].

mod auth;

use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

pub struct CodexChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for CodexChannel {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::OpenAiResponses
    }

    fn prepare(&self, _ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        Err(ChannelError::Unsupported(
            "codex channel: OAuth + Responses/compact not implemented yet (M7/M2)",
        ))
    }
}
