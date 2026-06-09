//! Kiro channel (Amazon Q, OAuth social/IdC; AWS Smithy event-stream). STUB —
//! registered so `kiro` resolves, but `prepare` errors until the OAuth refresh
//! infra (M7) and the Smithy event-stream transform (M2) land. See [`auth`].

mod auth;

use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

pub struct KiroChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for KiroChannel {
    fn id(&self) -> &'static str {
        "kiro"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::OpenAiResponses
    }

    fn prepare(&self, _ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        Err(ChannelError::Unsupported(
            "kiro channel: OAuth + Smithy event-stream not implemented yet (M7/M2)",
        ))
    }
}
