//! Vertex AI channel (Google Cloud, service-account JWT → bearer token).
//!
//! **STUB**: registered so the `vertex` id resolves, but `prepare` errors until
//! the OAuth refresh infrastructure lands (M7). See [`auth`] for the intended
//! mechanism.

mod auth;

use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

pub struct VertexChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for VertexChannel {
    fn id(&self) -> &'static str {
        "vertex"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::GeminiGenerateContent
    }

    fn prepare(&self, _ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        Err(ChannelError::Unsupported(
            "vertex channel: OAuth service-account auth not implemented yet (M7)",
        ))
    }
}
