pub use super::constants::{
    DEFAULT_BASE_URL, DEFAULT_CLAUDE_AI_BASE_URL, DEFAULT_PLATFORM_BASE_URL, OAUTH_BETA,
};

use serde::{Deserialize, Serialize};

use crate::channels::cache_control::{CacheBreakpointRule, parse_cache_breakpoint_rules};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeCodeSettings {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    pub claude_ai_base_url: String,
    pub platform_base_url: String,
    pub prelude_text: Option<String>,
    #[serde(default)]
    pub append_beta_query: bool,
    #[serde(default)]
    pub extra_beta_headers: Vec<String>,
    #[serde(default)]
    pub cache_breakpoints: Vec<CacheBreakpointRule>,
}

impl Default for ClaudeCodeSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            user_agent: None,
            claude_ai_base_url: DEFAULT_CLAUDE_AI_BASE_URL.to_string(),
            platform_base_url: DEFAULT_PLATFORM_BASE_URL.to_string(),
            prelude_text: None,
            append_beta_query: false,
            extra_beta_headers: Vec::new(),
            cache_breakpoints: Vec::new(),
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
        settings.append_beta_query = parse_bool_flag(value.get("claudecode_append_beta_query"));
        settings.extra_beta_headers =
            parse_extra_beta_headers(value.get("claudecode_extra_beta_headers"));
        settings.cache_breakpoints = parse_cache_breakpoint_rules(value.get("cache_breakpoints"));
        Ok(settings)
    }
}

fn clean_opt(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn parse_bool_flag(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::Bool(value)) => *value,
        Some(serde_json::Value::String(value)) => value.trim().eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn parse_extra_beta_headers(value: Option<&serde_json::Value>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(value) = value else {
        return out;
    };

    match value {
        serde_json::Value::String(raw) => {
            collect_beta_values(raw.split(','), &mut out);
        }
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(raw) = item.as_str() {
                    collect_beta_values(raw.split(','), &mut out);
                }
            }
        }
        _ => {}
    }

    out
}

fn collect_beta_values<'a>(values: impl IntoIterator<Item = &'a str>, out: &mut Vec<String>) {
    for raw in values {
        let value = raw.trim();
        if value.is_empty() || value.eq_ignore_ascii_case(OAUTH_BETA) {
            continue;
        }
        if !out
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(value))
        {
            out.push(value.to_string());
        }
    }
}
