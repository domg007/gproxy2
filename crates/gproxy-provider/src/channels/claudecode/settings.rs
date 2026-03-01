pub use super::constants::{
    DEFAULT_BASE_URL, DEFAULT_CLAUDE_AI_BASE_URL, DEFAULT_PLATFORM_BASE_URL,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeCodeSettings {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    pub claude_ai_base_url: String,
    pub platform_base_url: String,
    pub prelude_text: Option<String>,
}

impl Default for ClaudeCodeSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            user_agent: None,
            claude_ai_base_url: DEFAULT_CLAUDE_AI_BASE_URL.to_string(),
            platform_base_url: DEFAULT_PLATFORM_BASE_URL.to_string(),
            prelude_text: None,
        }
    }
}
