pub use super::constants::DEFAULT_BASE_URL;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AntigravitySettings {
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

impl Default for AntigravitySettings {
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
