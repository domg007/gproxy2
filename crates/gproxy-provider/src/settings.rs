use crate::channel::{BuiltinChannel, ChannelId};
use crate::channels::{
    BuiltinChannelSettings, ChannelSettings, aistudio, claude, custom, deepseek, groq, nvidia,
    openai, vertexexpress,
};

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
    Ok(match channel {
        ChannelId::Builtin(BuiltinChannel::OpenAi) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::OpenAi(
                openai::OpenAiSettings::from_provider_settings_value(value)?,
            ))
        }
        ChannelId::Builtin(BuiltinChannel::Claude) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::Claude(
                claude::ClaudeSettings::from_provider_settings_value(value)?,
            ))
        }
        ChannelId::Builtin(BuiltinChannel::AiStudio) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::AiStudio(
                aistudio::AiStudioSettings::from_provider_settings_value(value)?,
            ))
        }
        ChannelId::Builtin(BuiltinChannel::VertexExpress) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::VertexExpress(
                vertexexpress::VertexExpressSettings::from_provider_settings_value(value)?,
            ))
        }
        ChannelId::Builtin(BuiltinChannel::Vertex) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::Vertex(
                crate::channels::vertex::VertexSettings::from_provider_settings_value(value)?,
            ))
        }
        ChannelId::Builtin(BuiltinChannel::GeminiCli) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::GeminiCli(
                crate::channels::geminicli::GeminiCliSettings::from_provider_settings_value(value)?,
            ))
        }
        ChannelId::Builtin(BuiltinChannel::ClaudeCode) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::ClaudeCode(
                crate::channels::claudecode::ClaudeCodeSettings::from_provider_settings_value(
                    value,
                )?,
            ))
        }
        ChannelId::Builtin(BuiltinChannel::Codex) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::Codex(
                crate::channels::codex::CodexSettings::from_provider_settings_value(value)?,
            ))
        }
        ChannelId::Builtin(BuiltinChannel::Antigravity) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::Antigravity(
                crate::channels::antigravity::AntigravitySettings::from_provider_settings_value(
                    value,
                )?,
            ))
        }
        ChannelId::Builtin(BuiltinChannel::Nvidia) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::Nvidia(
                nvidia::NvidiaSettings::from_provider_settings_value(value)?,
            ))
        }
        ChannelId::Builtin(BuiltinChannel::Deepseek) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::Deepseek(
                deepseek::DeepseekSettings::from_provider_settings_value(value)?,
            ))
        }
        ChannelId::Builtin(BuiltinChannel::Groq) => ChannelSettings::Builtin(
            BuiltinChannelSettings::Groq(groq::GroqSettings::from_provider_settings_value(value)?),
        ),
        ChannelId::Custom(_) => ChannelSettings::Custom(
            custom::CustomChannelSettings::from_provider_settings_value(value)?,
        ),
    })
}

pub fn provider_settings_to_json_value(settings: &ChannelSettings) -> serde_json::Value {
    let mut root = serde_json::Map::new();
    root.insert(
        "base_url".to_string(),
        serde_json::Value::String(settings.base_url().to_string()),
    );
    if let Some(user_agent) = settings.user_agent() {
        root.insert(
            "user_agent".to_string(),
            serde_json::Value::String(user_agent.trim().to_string()),
        );
    }

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
