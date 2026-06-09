//! Claude Code channel (Anthropic OAuth2 + PKCE, cookie fallback, TLS
//! emulation). STUB — registered so `claudecode` resolves, but `prepare` errors
//! until the OAuth/cookie + TLS-emulation infra (M7) lands. See [`auth`].

mod auth;

use crate::channel::{Channel, ChannelError, PrepareCtx, PreparedRequest};
use crate::protocol::ContentGenerationKind;

pub struct ClaudeCodeChannel;

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Channel for ClaudeCodeChannel {
    fn id(&self) -> &'static str {
        "claudecode"
    }

    fn target_kind(&self) -> ContentGenerationKind {
        ContentGenerationKind::ClaudeMessages
    }

    fn default_tls_fingerprint(&self) -> Option<serde_json::Value> {
        // Impersonation-by-default. Placeholder profile; the concrete JA3/preset
        // shape lands with the emulation transport (M7).
        Some(serde_json::json!({ "profile": "claude_cli" }))
    }

    fn prepare(&self, _ctx: PrepareCtx<'_>) -> Result<PreparedRequest, ChannelError> {
        Err(ChannelError::Unsupported(
            "claudecode channel: OAuth/cookie + TLS emulation not implemented yet (M7)",
        ))
    }
}
