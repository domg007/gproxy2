use crate::channel::{BuiltinChannel, ChannelId};
use crate::channels::cache_control::cache_breakpoint_rules_to_settings_value;
use crate::channels::{
    BuiltinChannelSettings, ChannelSettings, aistudio, anthropic, custom, deepseek, groq, nvidia,
    openai, retry::CredentialPickMode, vertexexpress,
};

pub const CREDENTIAL_PICK_MODE_KEY: &str = "credential_pick_mode";
pub const CREDENTIAL_ROUND_ROBIN_ENABLED_KEY: &str = "credential_round_robin_enabled";
pub const CREDENTIAL_CACHE_AFFINITY_ENABLED_KEY: &str = "credential_cache_affinity_enabled";
pub const CREDENTIAL_CACHE_AFFINITY_MAX_KEYS_KEY: &str = "credential_cache_affinity_max_keys";
pub const DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS: usize = 4096;

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

    CredentialPickMode::RoundRobinWithCache
}

pub fn parse_credential_cache_affinity_max_keys_from_provider_settings_value(
    value: &serde_json::Value,
) -> Result<usize, String> {
    let Some(raw) = value.get(CREDENTIAL_CACHE_AFFINITY_MAX_KEYS_KEY) else {
        return Ok(DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS);
    };

    let Some(parsed) = raw.as_u64() else {
        return Err(format!(
            "{CREDENTIAL_CACHE_AFFINITY_MAX_KEYS_KEY} must be a positive integer"
        ));
    };
    if parsed == 0 {
        return Err(format!(
            "{CREDENTIAL_CACHE_AFFINITY_MAX_KEYS_KEY} must be at least 1"
        ));
    }
    usize::try_from(parsed).map_err(|_| {
        format!("{CREDENTIAL_CACHE_AFFINITY_MAX_KEYS_KEY} is too large for this platform")
    })
}

fn parse_credential_pick_mode_from_str(value: &str) -> Option<CredentialPickMode> {
    match value {
        "sticky_no_cache" => Some(CredentialPickMode::StickyNoCache),
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
        ChannelId::Builtin(BuiltinChannel::Anthropic) => {
            ChannelSettings::Builtin(BuiltinChannelSettings::Anthropic(
                anthropic::AnthropicSettings::from_provider_settings_value(value)?,
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

pub fn provider_settings_to_json_value_with_routing(
    settings: &ChannelSettings,
    credential_pick_mode: CredentialPickMode,
    cache_affinity_max_keys: usize,
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
        ChannelSettings::Builtin(BuiltinChannelSettings::Anthropic(value)) => {
            let prelude = value
                .prelude_text
                .as_deref()
                .map(str::trim)
                .unwrap_or_default();
            if !prelude.is_empty() {
                root.insert(
                    "anthropic_prelude_text".to_string(),
                    serde_json::Value::String(prelude.to_string()),
                );
            }
            if value.append_beta_query {
                root.insert(
                    "anthropic_append_beta_query".to_string(),
                    serde_json::Value::Bool(true),
                );
            }
            if !value.extra_beta_headers.is_empty() {
                root.insert(
                    "anthropic_extra_beta_headers".to_string(),
                    serde_json::Value::Array(
                        value
                            .extra_beta_headers
                            .iter()
                            .map(|item| serde_json::Value::String(item.clone()))
                            .collect(),
                    ),
                );
            }
            if let Some(rules) = cache_breakpoint_rules_to_settings_value(&value.cache_breakpoints)
            {
                root.insert("cache_breakpoints".to_string(), rules);
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
            if value.append_beta_query {
                root.insert(
                    "claudecode_append_beta_query".to_string(),
                    serde_json::Value::Bool(true),
                );
            }
            if !value.extra_beta_headers.is_empty() {
                root.insert(
                    "claudecode_extra_beta_headers".to_string(),
                    serde_json::Value::Array(
                        value
                            .extra_beta_headers
                            .iter()
                            .map(|item| serde_json::Value::String(item.clone()))
                            .collect(),
                    ),
                );
            }
            if let Some(rules) = cache_breakpoint_rules_to_settings_value(&value.cache_breakpoints)
            {
                root.insert("cache_breakpoints".to_string(), rules);
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
    root.insert(
        CREDENTIAL_CACHE_AFFINITY_MAX_KEYS_KEY.to_string(),
        serde_json::Value::from(cache_affinity_max_keys as u64),
    );
    // Keep for backward compatibility with older readers.
    root.insert(
        CREDENTIAL_PICK_MODE_KEY.to_string(),
        serde_json::Value::String(credential_pick_mode_to_str(credential_pick_mode).to_string()),
    );

    serde_json::Value::Object(root)
}

pub fn provider_settings_to_json_value_with_credential_pick_mode(
    settings: &ChannelSettings,
    credential_pick_mode: CredentialPickMode,
) -> serde_json::Value {
    provider_settings_to_json_value_with_routing(
        settings,
        credential_pick_mode,
        DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS,
    )
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

pub fn provider_settings_to_json_string_with_routing(
    settings: &ChannelSettings,
    credential_pick_mode: CredentialPickMode,
    cache_affinity_max_keys: usize,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&provider_settings_to_json_value_with_routing(
        settings,
        credential_pick_mode,
        cache_affinity_max_keys,
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
        CREDENTIAL_CACHE_AFFINITY_ENABLED_KEY, CREDENTIAL_CACHE_AFFINITY_MAX_KEYS_KEY,
        CREDENTIAL_PICK_MODE_KEY, CREDENTIAL_ROUND_ROBIN_ENABLED_KEY,
        DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS,
        parse_credential_cache_affinity_max_keys_from_provider_settings_value,
        parse_credential_pick_mode_from_provider_settings_value,
        provider_settings_to_json_value_with_credential_pick_mode,
        provider_settings_to_json_value_with_routing,
    };
    use crate::channels::cache_control::{
        CacheBreakpointPositionKind, CacheBreakpointRule, CacheBreakpointTarget, CacheBreakpointTtl,
    };
    use crate::channels::retry::CredentialPickMode;
    use crate::channels::settings::{BuiltinChannelSettings, ChannelSettings};
    use crate::channels::{anthropic, claudecode};

    #[test]
    fn credential_pick_mode_defaults_round_robin_with_cache() {
        assert_eq!(
            parse_credential_pick_mode_from_provider_settings_value(&serde_json::json!({})),
            CredentialPickMode::RoundRobinWithCache
        );
    }

    #[test]
    fn parse_legacy_bool_is_ignored() {
        assert_eq!(
            parse_credential_pick_mode_from_provider_settings_value(
                &serde_json::json!({ "cache_affinity_enabled": false })
            ),
            CredentialPickMode::RoundRobinWithCache
        );
    }

    #[test]
    fn parse_sticky_with_cache_is_ignored() {
        assert_eq!(
            parse_credential_pick_mode_from_provider_settings_value(
                &serde_json::json!({ "credential_pick_mode": "sticky_with_cache" })
            ),
            CredentialPickMode::RoundRobinWithCache
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
    fn credential_cache_affinity_max_keys_defaults_to_4096() {
        assert_eq!(
            parse_credential_cache_affinity_max_keys_from_provider_settings_value(
                &serde_json::json!({})
            )
            .expect("default max keys"),
            DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS
        );
    }

    #[test]
    fn parse_credential_cache_affinity_max_keys_requires_positive_integer() {
        assert_eq!(
            parse_credential_cache_affinity_max_keys_from_provider_settings_value(
                &serde_json::json!({ "credential_cache_affinity_max_keys": 1024 })
            )
            .expect("explicit max keys"),
            1024
        );
        assert!(
            parse_credential_cache_affinity_max_keys_from_provider_settings_value(
                &serde_json::json!({ "credential_cache_affinity_max_keys": 0 })
            )
            .is_err()
        );
        assert!(
            parse_credential_cache_affinity_max_keys_from_provider_settings_value(
                &serde_json::json!({ "credential_cache_affinity_max_keys": "4096" })
            )
            .is_err()
        );
    }

    #[test]
    fn serialize_settings_includes_pick_mode() {
        let value = provider_settings_to_json_value_with_routing(
            &ChannelSettings::default(),
            CredentialPickMode::RoundRobinNoCache,
            2048,
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
        assert_eq!(
            value
                .get(CREDENTIAL_CACHE_AFFINITY_MAX_KEYS_KEY)
                .and_then(serde_json::Value::as_u64),
            Some(2048)
        );
    }

    #[test]
    fn serialize_anthropic_settings_includes_cache_breakpoints() {
        let settings = ChannelSettings::Builtin(BuiltinChannelSettings::Anthropic(
            anthropic::AnthropicSettings {
                append_beta_query: true,
                cache_breakpoints: vec![
                    CacheBreakpointRule {
                        target: CacheBreakpointTarget::TopLevel,
                        position: CacheBreakpointPositionKind::Nth,
                        index: 1,
                        ttl: CacheBreakpointTtl::Auto,
                    },
                    CacheBreakpointRule {
                        target: CacheBreakpointTarget::Messages,
                        position: CacheBreakpointPositionKind::LastNth,
                        index: 1,
                        ttl: CacheBreakpointTtl::Ttl1h,
                    },
                ],
                ..Default::default()
            },
        ));

        let value = provider_settings_to_json_value_with_credential_pick_mode(
            &settings,
            CredentialPickMode::RoundRobinWithCache,
        );
        assert_eq!(
            value
                .get("cache_breakpoints")
                .and_then(serde_json::Value::as_array)
                .map(|items| items.len()),
            Some(2)
        );
        assert_eq!(
            value
                .get("anthropic_append_beta_query")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn serialize_claudecode_settings_includes_cache_breakpoints() {
        let settings = ChannelSettings::Builtin(BuiltinChannelSettings::ClaudeCode(
            claudecode::ClaudeCodeSettings {
                append_beta_query: true,
                cache_breakpoints: vec![CacheBreakpointRule {
                    target: CacheBreakpointTarget::System,
                    position: CacheBreakpointPositionKind::Nth,
                    index: 2,
                    ttl: CacheBreakpointTtl::Ttl5m,
                }],
                ..Default::default()
            },
        ));

        let value = provider_settings_to_json_value_with_credential_pick_mode(
            &settings,
            CredentialPickMode::RoundRobinWithCache,
        );
        assert_eq!(
            value
                .get("cache_breakpoints")
                .and_then(serde_json::Value::as_array)
                .map(|items| items.len()),
            Some(1)
        );
        assert_eq!(
            value
                .get("claudecode_append_beta_query")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }
}
