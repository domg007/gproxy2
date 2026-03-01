use crate::channel::{BuiltinChannel, ChannelId};
use crate::channels::antigravity::AntigravitySettings;
use crate::channels::claudecode::settings::ClaudeCodeSettings;
use crate::channels::codex::CodexSettings;
use crate::channels::geminicli::GeminiCliSettings;
use crate::channels::vertex::VertexSettings;
use crate::channels::{
    BuiltinChannelSettings, ChannelSettings, aistudio, claude, custom, deepseek, groq, nvidia,
    openai, vertexexpress,
};

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct LegacyProviderSettings {
    base_url: String,
    user_agent: Option<String>,
    oauth_issuer_url: Option<String>,
    oauth_authorize_url: Option<String>,
    oauth_token_url: Option<String>,
    oauth_userinfo_url: Option<String>,
    claudecode_ai_base_url: Option<String>,
    claudecode_platform_base_url: Option<String>,
    claudecode_prelude_text: Option<String>,
    mask_table: Option<serde_json::Value>,
}

pub fn parse_provider_settings_json_for_channel(
    channel: &ChannelId,
    raw_json: &str,
) -> Result<ChannelSettings, serde_json::Error> {
    let value = serde_json::from_str::<serde_json::Value>(raw_json)?;
    parse_provider_settings_value_for_channel(channel, &value)
}

pub fn parse_provider_settings_value_for_channel(
    channel: &ChannelId,
    value: &serde_json::Value,
) -> Result<ChannelSettings, serde_json::Error> {
    let legacy = serde_json::from_value::<LegacyProviderSettings>(value.clone())?;
    let user_agent = clean_opt(legacy.user_agent.as_deref()).map(ToOwned::to_owned);
    Ok(match channel {
        ChannelId::Builtin(BuiltinChannel::OpenAi) => {
            let mut settings = openai::OpenAiSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            ChannelSettings::Builtin(BuiltinChannelSettings::OpenAi(settings))
        }
        ChannelId::Builtin(BuiltinChannel::Claude) => {
            let mut settings = claude::ClaudeSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            ChannelSettings::Builtin(BuiltinChannelSettings::Claude(settings))
        }
        ChannelId::Builtin(BuiltinChannel::AiStudio) => {
            let mut settings = aistudio::AiStudioSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            ChannelSettings::Builtin(BuiltinChannelSettings::AiStudio(settings))
        }
        ChannelId::Builtin(BuiltinChannel::VertexExpress) => {
            let mut settings = vertexexpress::VertexExpressSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            ChannelSettings::Builtin(BuiltinChannelSettings::VertexExpress(settings))
        }
        ChannelId::Builtin(BuiltinChannel::Vertex) => {
            let mut settings = VertexSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            if let Some(oauth_token_url) = clean_opt(legacy.oauth_token_url.as_deref()) {
                settings.oauth_token_url = oauth_token_url.to_string();
            }
            ChannelSettings::Builtin(BuiltinChannelSettings::Vertex(settings))
        }
        ChannelId::Builtin(BuiltinChannel::GeminiCli) => {
            let mut settings = GeminiCliSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            settings.oauth_authorize_url =
                clean_opt(legacy.oauth_authorize_url.as_deref()).map(ToOwned::to_owned);
            settings.oauth_token_url =
                clean_opt(legacy.oauth_token_url.as_deref()).map(ToOwned::to_owned);
            settings.oauth_userinfo_url =
                clean_opt(legacy.oauth_userinfo_url.as_deref()).map(ToOwned::to_owned);
            ChannelSettings::Builtin(BuiltinChannelSettings::GeminiCli(settings))
        }
        ChannelId::Builtin(BuiltinChannel::ClaudeCode) => {
            let mut settings = ClaudeCodeSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            if let Some(value) = clean_opt(legacy.claudecode_ai_base_url.as_deref()) {
                settings.claude_ai_base_url = value.to_string();
            }
            if let Some(value) = clean_opt(legacy.claudecode_platform_base_url.as_deref()) {
                settings.platform_base_url = value.to_string();
            }
            settings.prelude_text =
                clean_opt(legacy.claudecode_prelude_text.as_deref()).map(ToOwned::to_owned);
            ChannelSettings::Builtin(BuiltinChannelSettings::ClaudeCode(settings))
        }
        ChannelId::Builtin(BuiltinChannel::Codex) => {
            let mut settings = CodexSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            settings.oauth_issuer_url =
                clean_opt(legacy.oauth_issuer_url.as_deref()).map(ToOwned::to_owned);
            ChannelSettings::Builtin(BuiltinChannelSettings::Codex(settings))
        }
        ChannelId::Builtin(BuiltinChannel::Antigravity) => {
            let mut settings = AntigravitySettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            settings.oauth_authorize_url =
                clean_opt(legacy.oauth_authorize_url.as_deref()).map(ToOwned::to_owned);
            settings.oauth_token_url =
                clean_opt(legacy.oauth_token_url.as_deref()).map(ToOwned::to_owned);
            settings.oauth_userinfo_url =
                clean_opt(legacy.oauth_userinfo_url.as_deref()).map(ToOwned::to_owned);
            ChannelSettings::Builtin(BuiltinChannelSettings::Antigravity(settings))
        }
        ChannelId::Builtin(BuiltinChannel::Nvidia) => {
            let mut settings = nvidia::NvidiaSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            ChannelSettings::Builtin(BuiltinChannelSettings::Nvidia(settings))
        }
        ChannelId::Builtin(BuiltinChannel::Deepseek) => {
            let mut settings = deepseek::DeepseekSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            ChannelSettings::Builtin(BuiltinChannelSettings::Deepseek(settings))
        }
        ChannelId::Builtin(BuiltinChannel::Groq) => {
            let mut settings = groq::GroqSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            ChannelSettings::Builtin(BuiltinChannelSettings::Groq(settings))
        }
        ChannelId::Custom(_) => {
            let mut settings = custom::CustomChannelSettings::default();
            if !legacy.base_url.trim().is_empty() {
                settings.base_url = legacy.base_url;
            }
            settings.user_agent = user_agent.clone();
            if let Some(mask_table) = legacy.mask_table.as_ref() {
                settings.mask_table = parse_custom_mask_table(mask_table)?;
            }
            ChannelSettings::Custom(settings)
        }
    })
}

pub fn provider_settings_to_json_value(settings: &ChannelSettings) -> serde_json::Value {
    let mut root = serde_json::Map::new();
    root.insert(
        "base_url".to_string(),
        serde_json::Value::String(settings.base_url().to_string()),
    );
    maybe_insert_opt_string(&mut root, "user_agent", settings.user_agent());

    match settings {
        ChannelSettings::Builtin(BuiltinChannelSettings::Codex(value)) => {
            if let Some(url) = clean_opt(value.oauth_issuer_url.as_deref()) {
                root.insert(
                    "oauth_issuer_url".to_string(),
                    serde_json::Value::String(url.to_string()),
                );
            }
        }
        ChannelSettings::Builtin(BuiltinChannelSettings::GeminiCli(value)) => {
            maybe_insert_opt_string(
                &mut root,
                "oauth_authorize_url",
                value.oauth_authorize_url.as_deref(),
            );
            maybe_insert_opt_string(
                &mut root,
                "oauth_token_url",
                value.oauth_token_url.as_deref(),
            );
            maybe_insert_opt_string(
                &mut root,
                "oauth_userinfo_url",
                value.oauth_userinfo_url.as_deref(),
            );
        }
        ChannelSettings::Builtin(BuiltinChannelSettings::Antigravity(value)) => {
            maybe_insert_opt_string(
                &mut root,
                "oauth_authorize_url",
                value.oauth_authorize_url.as_deref(),
            );
            maybe_insert_opt_string(
                &mut root,
                "oauth_token_url",
                value.oauth_token_url.as_deref(),
            );
            maybe_insert_opt_string(
                &mut root,
                "oauth_userinfo_url",
                value.oauth_userinfo_url.as_deref(),
            );
        }
        ChannelSettings::Builtin(BuiltinChannelSettings::Vertex(value)) => {
            if !value.oauth_token_url.trim().is_empty() {
                root.insert(
                    "oauth_token_url".to_string(),
                    serde_json::Value::String(value.oauth_token_url.trim().to_string()),
                );
            }
        }
        ChannelSettings::Builtin(BuiltinChannelSettings::ClaudeCode(value)) => {
            if !value.claude_ai_base_url.trim().is_empty() {
                root.insert(
                    "claudecode_ai_base_url".to_string(),
                    serde_json::Value::String(value.claude_ai_base_url.trim().to_string()),
                );
            }
            if !value.platform_base_url.trim().is_empty() {
                root.insert(
                    "claudecode_platform_base_url".to_string(),
                    serde_json::Value::String(value.platform_base_url.trim().to_string()),
                );
            }
            let prelude = value
                .prelude_text
                .as_deref()
                .map(str::trim)
                .unwrap_or_default();
            root.insert(
                "claudecode_prelude_text".to_string(),
                serde_json::Value::String(prelude.to_string()),
            );
        }
        ChannelSettings::Custom(value) => {
            if !value.mask_table.rules.is_empty()
                && let Ok(mask_value) = serde_json::to_value(&value.mask_table)
            {
                root.insert("mask_table".to_string(), mask_value);
            }
        }
        _ => {}
    }

    serde_json::Value::Object(root)
}

pub fn provider_settings_to_json_string(
    settings: &ChannelSettings,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&provider_settings_to_json_value(settings))
}

fn maybe_insert_opt_string(
    target: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = clean_opt(value) {
        target.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
}

fn clean_opt(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn parse_custom_mask_table(
    value: &serde_json::Value,
) -> Result<custom::settings::CustomMaskTable, serde_json::Error> {
    match value {
        serde_json::Value::Null => Ok(custom::settings::CustomMaskTable::default()),
        serde_json::Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Ok(custom::settings::CustomMaskTable::default());
            }
            let parsed = serde_json::from_str::<serde_json::Value>(trimmed)?;
            serde_json::from_value(parsed)
        }
        _ => serde_json::from_value(value.clone()),
    }
}
