pub use super::constants::DEFAULT_BASE_URL;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeminiCliSettings {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_authorize_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_token_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_userinfo_url: Option<String>,
}

impl Default for GeminiCliSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            user_agent: None,
            oauth_authorize_url: None,
            oauth_token_url: None,
            oauth_userinfo_url: None,
        }
    }
}

impl GeminiCliSettings {
    pub fn from_provider_settings_value(
        value: &serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        #[derive(Debug, Clone, Default, Deserialize)]
        #[serde(default)]
        struct ProviderSettingsPatch {
            base_url: String,
            user_agent: Option<String>,
            oauth_authorize_url: Option<String>,
            oauth_token_url: Option<String>,
            oauth_userinfo_url: Option<String>,
        }

        let patch = serde_json::from_value::<ProviderSettingsPatch>(value.clone())?;
        let mut settings = Self::default();
        if !patch.base_url.trim().is_empty() {
            settings.base_url = patch.base_url;
        }
        settings.user_agent = patch.user_agent.map(|value| value.trim().to_string());
        settings.oauth_authorize_url =
            clean_opt(patch.oauth_authorize_url.as_deref()).map(ToOwned::to_owned);
        settings.oauth_token_url =
            clean_opt(patch.oauth_token_url.as_deref()).map(ToOwned::to_owned);
        settings.oauth_userinfo_url =
            clean_opt(patch.oauth_userinfo_url.as_deref()).map(ToOwned::to_owned);
        Ok(settings)
    }
}

fn clean_opt(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
