pub use super::constants::DEFAULT_BASE_URL;

use serde::{Deserialize, Serialize};

use crate::channels::cache_control::{
    CacheBreakpointRule, parse_cache_breakpoint_rules,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeSettings {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub cache_breakpoints: Vec<CacheBreakpointRule>,
}

impl Default for ClaudeSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            user_agent: None,
            cache_breakpoints: Vec::new(),
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
        settings.cache_breakpoints = parse_cache_breakpoint_rules(value.get("cache_breakpoints"));
        Ok(settings)
    }
}
