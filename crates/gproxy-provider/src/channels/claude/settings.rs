pub use super::constants::DEFAULT_BASE_URL;

use serde::{Deserialize, Serialize};

use crate::channels::cache_control::TopLevelCacheControlMode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeSettings {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub top_level_cache_control_mode: TopLevelCacheControlMode,
}

impl Default for ClaudeSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            user_agent: None,
            top_level_cache_control_mode: TopLevelCacheControlMode::Disabled,
        }
    }
}

impl ClaudeSettings {
    pub fn from_provider_settings_value(
        value: &serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        #[derive(Debug, Clone, Default, Deserialize)]
        #[serde(default)]
        struct ProviderSettingsPatch {
            base_url: String,
            user_agent: Option<String>,
        }

        let patch = serde_json::from_value::<ProviderSettingsPatch>(value.clone())?;
        let mut settings = Self::default();
        if !patch.base_url.trim().is_empty() {
            settings.base_url = patch.base_url;
        }
        settings.user_agent = patch.user_agent.map(|value| value.trim().to_string());
        settings.top_level_cache_control_mode = TopLevelCacheControlMode::from_settings_value(
            value.get("enable_top_level_cache_control"),
        );
        Ok(settings)
    }
}
