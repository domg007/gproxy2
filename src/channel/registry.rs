//! Startup-built `channel_id -> Arc<dyn Channel>` map (§6.3). No big match.
//!
//! Each channel is a folder under [`crate::channel::bulletins`] that manages its
//! own auth (`auth.rs`). The id (== `Provider.channel`) is the registry key.
//! API-key channels are functional; OAuth/envelope channels are registered as
//! stubs (their `prepare` errors until the M7 OAuth infra / M2 transforms land).

use std::collections::HashMap;
use std::sync::Arc;

use crate::channel::Channel;
use crate::channel::bulletins;

/// Registry of channel adapters keyed by `Channel::id` (== `Provider.channel`).
pub struct ChannelRegistry {
    map: HashMap<&'static str, Arc<dyn Channel>>,
}

impl ChannelRegistry {
    /// Build the full channel set. Pure `http` + `serde_json` logic; compiles on
    /// native AND wasm32.
    pub fn with_builtin() -> Self {
        let mut map: HashMap<&'static str, Arc<dyn Channel>> = HashMap::new();
        for ch in builtin_channels() {
            map.insert(ch.id(), ch);
        }
        Self { map }
    }

    /// Look up a channel by id.
    pub fn get(&self, id: &str) -> Option<Arc<dyn Channel>> {
        self.map.get(id).cloned()
    }

    /// Test-only: build the full built-in set plus one extra (or overriding)
    /// channel under `id`. Lets integration tests drive paths no built-in
    /// channel exercises (e.g. a channel whose `refresh` succeeds).
    #[cfg(test)]
    pub fn with_channel(id: &'static str, channel: Arc<dyn Channel>) -> Self {
        let mut reg = Self::with_builtin();
        reg.map.insert(id, channel);
        reg
    }
}

/// All built-in channel adapters (API-key functional; OAuth/envelope = stubs).
fn builtin_channels() -> Vec<Arc<dyn Channel>> {
    vec![
        // ── API-key (functional) ──
        Arc::new(bulletins::openai::OpenAiChannel),
        Arc::new(bulletins::openrouter::OpenRouterChannel),
        Arc::new(bulletins::deepseek::DeepSeekChannel),
        Arc::new(bulletins::groq::GroqChannel),
        Arc::new(bulletins::nvidia::NvidiaChannel),
        Arc::new(bulletins::vercel::VercelChannel),
        Arc::new(bulletins::custom::CustomChannel),
        Arc::new(bulletins::claude_api::ClaudeApiChannel),
        Arc::new(bulletins::aistudio::AiStudioChannel),
        Arc::new(bulletins::vertexexpress::VertexExpressChannel),
        // ── OAuth / envelope (stubs: prepare errors until M7/M2) ──
        Arc::new(bulletins::vertex::VertexChannel),
        Arc::new(bulletins::geminicli::GeminiCliChannel),
        Arc::new(bulletins::antigravity::AntigravityChannel),
        Arc::new(bulletins::claudecode::ClaudeCodeChannel),
        Arc::new(bulletins::codex::CodexChannel),
        Arc::new(bulletins::kiro::KiroChannel),
        Arc::new(bulletins::copilot_cli::CopilotCliChannel),
    ]
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::with_builtin()
    }
}
