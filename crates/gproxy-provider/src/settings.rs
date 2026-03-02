use crate::channel::{BuiltinChannel, ChannelId};
use crate::channels::{
    BuiltinChannelSettings, ChannelSettings, aistudio, claude, custom, deepseek, groq, nvidia,
    openai, retry::CredentialPickMode, vertexexpress,
};

pub const CACHE_AFFINITY_ENABLED_KEY: &str = "cache_affinity_enabled";
pub const CREDENTIAL_PICK_MODE_KEY: &str = "credential_pick_mode";
pub const CREDENTIAL_ROUND_ROBIN_ENABLED_KEY: &str = "credential_round_robin_enabled";
pub const CREDENTIAL_CACHE_AFFINITY_ENABLED_KEY: &str = "credential_cache_affinity_enabled";

pub fn parse_credential_pick_mode_from_provider_settings_value(
    value: &serde_json::Value,
) -> CredentialPickMode {
    let round_robin_enabled = value
        .get(CREDENTIAL_ROUND_ROBIN_ENABLED_KEY)
        .and_then(serde_json::Value::as_bool);
    let cache_affinity_enabled = value
        .get(CREDENTIAL_CACHE_AFFINITY_ENABLED_KEY)
        .and_then(serde_json::Value::as_bool);
    if round_robin_enabled.is_some() || cache_affinity_enabled.is_some() {
        return credential_pick_mode_from_bools(
            round_robin_enabled.unwrap_or(true),
            cache_affinity_enabled.unwrap_or(true),
        );
    }

    if let Some(mode) = value
        .get(CREDENTIAL_PICK_MODE_KEY)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .and_then(parse_credential_pick_mode_from_str)
    {
        return mode;
    }

    let legacy_cache_affinity_enabled = value
        .get(CACHE_AFFINITY_ENABLED_KEY)
        .and_then(serde_json::Value::as_bool);
    match legacy_cache_affinity_enabled {
        Some(false) => CredentialPickMode::StickyNoCache,
        Some(true) => CredentialPickMode::RoundRobinWithCache,
        None => CredentialPickMode::RoundRobinWithCache,
    }
}

fn parse_credential_pick_mode_from_str(value: &str) -> Option<CredentialPickMode> {
    match value {
        "sticky_no_cache" => Some(CredentialPickMode::StickyNoCache),
        // Legacy mode. We no longer support sticky+cache affinity.
        "sticky_with_cache" => Some(CredentialPickMode::StickyNoCache),
        "round_robin_with_cache" => Some(CredentialPickMode::RoundRobinWithCache),
        "round_robin_no_cache" => Some(CredentialPickMode::RoundRobinNoCache),
        _ => None,
    }
}

fn credential_pick_mode_from_bools(
    round_robin_enabled: bool,
    cache_affinity_enabled: bool,
) -> CredentialPickMode {
    if round_robin_enabled {
        if cache_affinity_enabled {
            CredentialPickMode::RoundRobinWithCache
        } else {
            CredentialPickMode::RoundRobinNoCache
        }
    } else {
        CredentialPickMode::StickyNoCache
    }
}

fn credential_pick_mode_to_str(mode: CredentialPickMode) -> &'static str {
    match mode {
        CredentialPickMode::StickyNoCache => "sticky_no_cache",
        CredentialPickMode::RoundRobinWithCache => "round_robin_with_cache",
        CredentialPickMode::RoundRobinNoCache => "round_robin_no_cache",
    }
}

fn credential_pick_mode_to_bools(mode: CredentialPickMode) -> (bool, bool) {
    match mode {
        CredentialPickMode::StickyNoCache => (false, false),
        CredentialPickMode::RoundRobinWithCache => (true, true),
        CredentialPickMode::RoundRobinNoCache => (true, false),
    }
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

pub fn provider_settings_to_json_value_with_credential_pick_mode(
    settings: &ChannelSettings,
    credential_pick_mode: CredentialPickMode,
) -> serde_json::Value {
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
        ChannelSettings::Builtin(BuiltinChannelSettings::Claude(value)) => {
            if value.enable_top_level_cache_control {
                root.insert(
                    "enable_top_level_cache_control".to_string(),
                    serde_json::Value::Bool(true),
                );
            }
        }
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
            if value.enable_top_level_cache_control {
                root.insert(
                    "enable_top_level_cache_control".to_string(),
                    serde_json::Value::Bool(true),
                );
            }
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

    let (round_robin_enabled, cache_affinity_enabled) =
        credential_pick_mode_to_bools(credential_pick_mode);
    root.insert(
        CREDENTIAL_ROUND_ROBIN_ENABLED_KEY.to_string(),
        serde_json::Value::Bool(round_robin_enabled),
    );
    root.insert(
        CREDENTIAL_CACHE_AFFINITY_ENABLED_KEY.to_string(),
        serde_json::Value::Bool(cache_affinity_enabled),
    );
    // Keep for backward compatibility with older readers.
    root.insert(
        CREDENTIAL_PICK_MODE_KEY.to_string(),
        serde_json::Value::String(credential_pick_mode_to_str(credential_pick_mode).to_string()),
    );

    serde_json::Value::Object(root)
}

pub fn provider_settings_to_json_value(settings: &ChannelSettings) -> serde_json::Value {
    provider_settings_to_json_value_with_credential_pick_mode(
        settings,
        CredentialPickMode::RoundRobinWithCache,
    )
}

pub fn provider_settings_to_json_string_with_credential_pick_mode(
    settings: &ChannelSettings,
    credential_pick_mode: CredentialPickMode,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&provider_settings_to_json_value_with_credential_pick_mode(
        settings,
        credential_pick_mode,
    ))
}

pub fn provider_settings_to_json_value_with_cache_affinity(
    settings: &ChannelSettings,
    cache_affinity_enabled: bool,
) -> serde_json::Value {
    let mode = if cache_affinity_enabled {
        CredentialPickMode::RoundRobinWithCache
    } else {
        CredentialPickMode::StickyNoCache
    };
    provider_settings_to_json_value_with_credential_pick_mode(settings, mode)
}

pub fn provider_settings_to_json_string_with_cache_affinity(
    settings: &ChannelSettings,
    cache_affinity_enabled: bool,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&provider_settings_to_json_value_with_cache_affinity(
        settings,
        cache_affinity_enabled,
    ))
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

#[cfg(test)]
mod tests {
    use super::{
        CREDENTIAL_CACHE_AFFINITY_ENABLED_KEY, CREDENTIAL_PICK_MODE_KEY,
        CREDENTIAL_ROUND_ROBIN_ENABLED_KEY,
        parse_credential_pick_mode_from_provider_settings_value,
        provider_settings_to_json_value_with_credential_pick_mode,
    };
    use crate::channels::retry::CredentialPickMode;
    use crate::channels::settings::ChannelSettings;

    #[test]
    fn credential_pick_mode_defaults_round_robin_with_cache() {
        assert_eq!(
            parse_credential_pick_mode_from_provider_settings_value(&serde_json::json!({})),
            CredentialPickMode::RoundRobinWithCache
        );
    }

    #[test]
    fn parse_legacy_bool_works() {
        assert_eq!(
            parse_credential_pick_mode_from_provider_settings_value(
                &serde_json::json!({ "cache_affinity_enabled": false })
            ),
            CredentialPickMode::StickyNoCache
        );
    }

    #[test]
    fn parse_sticky_with_cache_downgrades_to_sticky_no_cache() {
        assert_eq!(
            parse_credential_pick_mode_from_provider_settings_value(
                &serde_json::json!({ "credential_pick_mode": "sticky_with_cache" })
            ),
            CredentialPickMode::StickyNoCache
        );
    }

    #[test]
    fn parse_two_bools_prefers_three_valid_combinations() {
        assert_eq!(
            parse_credential_pick_mode_from_provider_settings_value(&serde_json::json!({
                "credential_round_robin_enabled": true,
                "credential_cache_affinity_enabled": true
            })),
            CredentialPickMode::RoundRobinWithCache
        );
        assert_eq!(
            parse_credential_pick_mode_from_provider_settings_value(&serde_json::json!({
                "credential_round_robin_enabled": true,
                "credential_cache_affinity_enabled": false
            })),
            CredentialPickMode::RoundRobinNoCache
        );
        assert_eq!(
            parse_credential_pick_mode_from_provider_settings_value(&serde_json::json!({
                "credential_round_robin_enabled": false,
                "credential_cache_affinity_enabled": true
            })),
            CredentialPickMode::StickyNoCache
        );
    }

    #[test]
    fn serialize_settings_includes_pick_mode() {
        let value = provider_settings_to_json_value_with_credential_pick_mode(
            &ChannelSettings::default(),
            CredentialPickMode::RoundRobinNoCache,
        );
        assert_eq!(
            value
                .get(CREDENTIAL_PICK_MODE_KEY)
                .and_then(serde_json::Value::as_str),
            Some("round_robin_no_cache")
        );
        assert_eq!(
            value
                .get(CREDENTIAL_ROUND_ROBIN_ENABLED_KEY)
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            value
                .get(CREDENTIAL_CACHE_AFFINITY_ENABLED_KEY)
                .and_then(serde_json::Value::as_bool),
            Some(false)
        );
    }
}
