//! Startup-built `channel_id -> Arc<dyn Channel>` map (§6.3). No big match.
//!
//! **Adapter boundary = authentication mechanism.** Each channel manages its own
//! auth (`Channel::prepare` injects it; `Channel::refresh` renews it per
//! adapter). Vendors that differ only by base URL but share an auth + wire
//! format collapse into ONE adapter, configured via `Provider.settings_json`:
//! e.g. `openai_compatible` (Bearer api-key) serves OpenAI, OpenRouter,
//! DeepSeek, Groq, NVIDIA, Vercel, and any custom OpenAI-compatible endpoint.
//! Channels with a distinct auth scheme get their own self-managing adapter
//! (`claude_api` = `x-api-key`; later: `gemini_api` = api-key; the OAuth /
//! cookie / TLS-emulation channels — vertex, geminicli, claudecode, codex,
//! chatgpt, antigravity, kiro — each as its own adapter). The set grows per
//! channel as those auth schemes land (M7 / §12); M1 ships the two API-key
//! adapters below.

use std::collections::HashMap;
use std::sync::Arc;

use crate::channel::Channel;
use crate::channel::claude_api::ClaudeApiChannel;
use crate::channel::openai_compatible::OpenAiCompatChannel;

/// Registry of channel adapters keyed by `Channel::id` (== `Provider.channel`).
pub struct ChannelRegistry {
    map: HashMap<&'static str, Arc<dyn Channel>>,
}

impl ChannelRegistry {
    /// Build with the M1 channel set. Compiles on native AND wasm32 (the
    /// channels are pure `http` + `serde_json` logic).
    pub fn with_builtin() -> Self {
        let mut map: HashMap<&'static str, Arc<dyn Channel>> = HashMap::new();
        for ch in [
            Arc::new(OpenAiCompatChannel) as Arc<dyn Channel>,
            Arc::new(ClaudeApiChannel) as Arc<dyn Channel>,
        ] {
            map.insert(ch.id(), ch);
        }
        Self { map }
    }

    /// Look up a channel by id.
    pub fn get(&self, id: &str) -> Option<Arc<dyn Channel>> {
        self.map.get(id).cloned()
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::with_builtin()
    }
}
