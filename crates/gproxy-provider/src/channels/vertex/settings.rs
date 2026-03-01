pub use super::constants::{DEFAULT_BASE_URL, DEFAULT_LOCATION, DEFAULT_TOKEN_URI};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VertexSettings {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    pub location: String,
    pub token_uri: String,
    pub oauth_token_url: String,
}

impl Default for VertexSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            user_agent: None,
            location: DEFAULT_LOCATION.to_string(),
            token_uri: DEFAULT_TOKEN_URI.to_string(),
            oauth_token_url: DEFAULT_TOKEN_URI.to_string(),
        }
    }
}

impl VertexSettings {
    pub fn from_provider_settings_value(
        value: &serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        #[derive(Debug, Clone, Default, Deserialize)]
        #[serde(default)]
        struct ProviderSettingsPatch {
            base_url: String,
            user_agent: Option<String>,
            oauth_token_url: Option<String>,
        }

        let patch = serde_json::from_value::<ProviderSettingsPatch>(value.clone())?;
        let mut settings = Self::default();
        if !patch.base_url.trim().is_empty() {
            settings.base_url = patch.base_url;
        }
        settings.user_agent = patch.user_agent.map(|value| value.trim().to_string());
        if let Some(oauth_token_url) = clean_opt(patch.oauth_token_url.as_deref()) {
            settings.oauth_token_url = oauth_token_url.to_string();
        }
        Ok(settings)
    }
}

fn clean_opt(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
