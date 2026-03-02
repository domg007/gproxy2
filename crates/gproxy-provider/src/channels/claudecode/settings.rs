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
    #[serde(default)]
    pub enable_top_level_cache_control: bool,
}

impl Default for ClaudeCodeSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            user_agent: None,
            claude_ai_base_url: DEFAULT_CLAUDE_AI_BASE_URL.to_string(),
            platform_base_url: DEFAULT_PLATFORM_BASE_URL.to_string(),
            prelude_text: None,
            enable_top_level_cache_control: false,
        }
    }
}

impl ClaudeCodeSettings {
    pub fn from_provider_settings_value(
        value: &serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        #[derive(Debug, Clone, Default, Deserialize)]
        #[serde(default)]
        struct ProviderSettingsPatch {
            base_url: String,
            user_agent: Option<String>,
            claudecode_ai_base_url: Option<String>,
            claudecode_platform_base_url: Option<String>,
            claudecode_prelude_text: Option<String>,
            enable_top_level_cache_control: bool,
        }

        let patch = serde_json::from_value::<ProviderSettingsPatch>(value.clone())?;
        let mut settings = Self::default();
        if !patch.base_url.trim().is_empty() {
            settings.base_url = patch.base_url;
        }
        settings.user_agent = patch.user_agent.map(|value| value.trim().to_string());
        if let Some(value) = clean_opt(patch.claudecode_ai_base_url.as_deref()) {
            settings.claude_ai_base_url = value.to_string();
        }
        if let Some(value) = clean_opt(patch.claudecode_platform_base_url.as_deref()) {
            settings.platform_base_url = value.to_string();
        }
        settings.prelude_text =
            clean_opt(patch.claudecode_prelude_text.as_deref()).map(ToOwned::to_owned);
        settings.enable_top_level_cache_control = patch.enable_top_level_cache_control;
        Ok(settings)
    }
}

fn clean_opt(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
