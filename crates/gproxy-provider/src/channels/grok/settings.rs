pub use super::constants::DEFAULT_BASE_URL;

use serde::{Deserialize, Serialize};

pub const DEFAULT_CF_SOLVER_TIMEOUT_SECONDS: u64 = 60;
pub const DEFAULT_CF_SESSION_TTL_SECONDS: u64 = 1800;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrokSettings {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cf_solver_url: Option<String>,
    #[serde(default = "default_cf_solver_timeout_seconds")]
    pub cf_solver_timeout_seconds: u64,
    #[serde(default = "default_cf_session_ttl_seconds")]
    pub cf_session_ttl_seconds: u64,
    #[serde(default)]
    pub temporary: bool,
    #[serde(default)]
    pub disable_memory: bool,
}

impl Default for GrokSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            user_agent: None,
            cf_solver_url: None,
            cf_solver_timeout_seconds: DEFAULT_CF_SOLVER_TIMEOUT_SECONDS,
            cf_session_ttl_seconds: DEFAULT_CF_SESSION_TTL_SECONDS,
            temporary: false,
            disable_memory: false,
        }
    }
}

impl GrokSettings {
    pub fn from_provider_settings_value(
        value: &serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        #[derive(Debug, Clone, Default, Deserialize)]
        #[serde(default)]
        struct ProviderSettingsPatch {
            base_url: String,
            user_agent: Option<String>,
            cf_solver_url: Option<String>,
            cf_solver_timeout_seconds: Option<u64>,
            cf_session_ttl_seconds: Option<u64>,
            temporary: bool,
            disable_memory: bool,
        }

        let patch = serde_json::from_value::<ProviderSettingsPatch>(value.clone())?;
        let mut settings = Self::default();
        if !patch.base_url.trim().is_empty() {
            settings.base_url = patch.base_url;
        }
        settings.user_agent = patch
            .user_agent
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        settings.cf_solver_url = patch
            .cf_solver_url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if let Some(value) = patch
            .cf_solver_timeout_seconds
            .filter(|value| *value > 0)
        {
            settings.cf_solver_timeout_seconds = value;
        }
        if let Some(value) = patch
            .cf_session_ttl_seconds
            .filter(|value| *value > 0)
        {
            settings.cf_session_ttl_seconds = value;
        }
        settings.temporary = patch.temporary;
        settings.disable_memory = patch.disable_memory;
        Ok(settings)
    }

    pub fn cf_solver_url(&self) -> Option<&str> {
        self.cf_solver_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn cf_solver_timeout_seconds(&self) -> u64 {
        self.cf_solver_timeout_seconds.max(1)
    }

    pub fn cf_session_ttl_seconds(&self) -> u64 {
        self.cf_session_ttl_seconds.max(1)
    }
}

const fn default_cf_solver_timeout_seconds() -> u64 {
    DEFAULT_CF_SOLVER_TIMEOUT_SECONDS
}

const fn default_cf_session_ttl_seconds() -> u64 {
    DEFAULT_CF_SESSION_TTL_SECONDS
}
