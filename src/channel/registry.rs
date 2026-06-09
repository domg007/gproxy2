//! Startup-built `channel_id -> Arc<dyn Channel>` map (§6.3). No big match.

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
