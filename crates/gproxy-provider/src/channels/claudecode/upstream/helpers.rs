use super::*;

pub(super) fn ensure_oauth_beta(headers: &mut Vec<(String, String)>, allow_context_1m: bool) {
    let values = normalized_claudecode_beta_values(
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
            .map(|(_, value)| parse_anthropic_beta_values(value))
            .unwrap_or_default(),
        allow_context_1m,
    );

    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("anthropic-beta"));
    headers.push(("anthropic-beta".to_string(), values.join(",")));
}

pub(super) fn is_context_1m_beta(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("context-1m")
}

pub(super) fn has_context_1m_beta(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| value.split(',').any(is_context_1m_beta))
        .unwrap_or(false)
}

pub(super) fn strip_context_1m_beta(headers: &mut Vec<(String, String)>) {
    let values = normalized_claudecode_beta_values(
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
            .map(|(_, value)| parse_anthropic_beta_values(value))
            .unwrap_or_default(),
        false,
    );

    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("anthropic-beta"));
    headers.push(("anthropic-beta".to_string(), values.join(",")));
}

pub(super) fn parse_anthropic_beta_values(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) fn normalized_claudecode_beta_values(
    mut values: Vec<String>,
    allow_context_1m: bool,
) -> Vec<String> {
    if !allow_context_1m {
        values.retain(|value| !is_context_1m_beta(value));
    }

    for required in std::iter::once(OAUTH_BETA).chain(CLAUDECODE_DEFAULT_BETAS.iter().copied()) {
        if !values
            .iter()
            .any(|value| value.eq_ignore_ascii_case(required))
        {
            values.push(required.to_string());
        }
    }

    values
}

pub(super) fn claude_1m_target_for_model(model: &str) -> Option<ClaudeCode1mTarget> {
    let lower = model.to_ascii_lowercase();
    if lower.starts_with("claude-sonnet-4") {
        return Some(ClaudeCode1mTarget::Sonnet);
    }
    if lower.starts_with("claude-opus-4-6") {
        return Some(ClaudeCode1mTarget::Opus);
    }
    None
}

pub(super) fn apply_claudecode_system(body: &mut Value, prelude_text: &str) {
    let Some(map) = body.as_object_mut() else {
        return;
    };

    if system_has_known_claudecode_prelude(map.get("system")) {
        return;
    }

    let prelude_block = json_text_block(prelude_text);
    match map.remove("system") {
        Some(Value::String(text)) => {
            map.insert(
                "system".to_string(),
                Value::Array(vec![prelude_block, json_text_block(text.as_str())]),
            );
        }
        Some(Value::Array(mut blocks)) => {
            blocks.insert(0, prelude_block);
            map.insert("system".to_string(), Value::Array(blocks));
        }
        Some(value) => {
            map.insert("system".to_string(), value);
        }
        None => {
            map.insert("system".to_string(), Value::Array(vec![prelude_block]));
        }
    }
}

pub(super) fn system_has_known_claudecode_prelude(system: Option<&Value>) -> bool {
    let Some(system) = system else {
        return false;
    };

    match system {
        Value::String(text) => is_known_claudecode_prelude_text(text),
        Value::Array(blocks) => blocks.iter().any(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(is_known_claudecode_prelude_text)
        }),
        _ => false,
    }
}

pub(super) fn is_known_claudecode_prelude_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("you are claude code") || lower.contains("claude agent sdk")
}

pub(super) fn json_text_block(text: &str) -> Value {
    serde_json::json!({
        "type": "text",
        "text": text,
    })
}

pub(super) fn claudecode_1m_enabled_for_credential(
    provider: &ProviderDefinition,
    credential_id: i64,
    target: Option<&ClaudeCode1mTarget>,
) -> bool {
    let Some(target) = target else {
        return false;
    };
    let Some(credential) = provider.credentials.credential(credential_id) else {
        return true;
    };
    let ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(value)) =
        &credential.credential
    else {
        return true;
    };

    match target {
        ClaudeCode1mTarget::Sonnet => value.enable_claude_1m_sonnet.unwrap_or(true),
        ClaudeCode1mTarget::Opus => value.enable_claude_1m_opus.unwrap_or(true),
    }
}

pub(super) fn disable_claudecode_1m_for_target(
    update: &mut Option<UpstreamCredentialUpdate>,
    credential_id: i64,
    target: Option<&ClaudeCode1mTarget>,
) {
    let Some(target) = target else {
        return;
    };

    let (disable_sonnet, disable_opus) = match target {
        ClaudeCode1mTarget::Sonnet => (Some(false), None),
        ClaudeCode1mTarget::Opus => (None, Some(false)),
    };

    if let Some(UpstreamCredentialUpdate::ClaudeCodeTokenRefresh {
        enable_claude_1m_sonnet,
        enable_claude_1m_opus,
        ..
    }) = update
    {
        if disable_sonnet.is_some() {
            *enable_claude_1m_sonnet = disable_sonnet;
        }
        if disable_opus.is_some() {
            *enable_claude_1m_opus = disable_opus;
        }
        return;
    }

    *update = Some(UpstreamCredentialUpdate::ClaudeCodeTokenRefresh {
        credential_id,
        access_token: None,
        refresh_token: None,
        expires_at_unix_ms: None,
        subscription_type: None,
        rate_limit_tier: None,
        user_email: None,
        cookie: None,
        enable_claude_1m_sonnet: disable_sonnet,
        enable_claude_1m_opus: disable_opus,
    });
}

pub(super) fn normalize_claudecode_sampling(model: &str, body: &mut Value) {
    let Some(map) = body.as_object_mut() else {
        return;
    };

    let has_temperature = map.get("temperature").and_then(Value::as_f64).is_some();
    let has_top_p = map.get("top_p").and_then(Value::as_f64).is_some();
    if has_temperature && has_top_p && requires_claudecode_sampling_guard(model) {
        map.remove("top_p");
    }
}

pub(super) fn normalize_claudecode_unsupported_fields(body: &mut Value) {
    let Some(map) = body.as_object_mut() else {
        return;
    };

    // Anthropic v1/messages on this upstream path currently rejects this field.
    map.remove("context_management");
}

pub(super) fn normalize_claudecode_model_and_thinking(model: &str, body: &mut Value) -> String {
    let trimmed = model.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with(CLAUDECODE_ADAPTIVE_THINKING_MODEL_SUFFIX) {
        let mut normalized = trimmed
            [..trimmed.len() - CLAUDECODE_ADAPTIVE_THINKING_MODEL_SUFFIX.len()]
            .trim()
            .to_string();
        if normalized.is_empty() {
            normalized = trimmed.to_string();
        }
        let Some(map) = body.as_object_mut() else {
            return normalized;
        };
        map.insert("model".to_string(), Value::String(normalized.clone()));
        map.insert(
            "thinking".to_string(),
            serde_json::json!({
                "type": "adaptive"
            }),
        );
        return normalized;
    }

    if lower.ends_with(CLAUDECODE_THINKING_MODEL_SUFFIX) {
        let mut normalized = trimmed[..trimmed.len() - CLAUDECODE_THINKING_MODEL_SUFFIX.len()]
            .trim()
            .to_string();
        if normalized.is_empty() {
            normalized = trimmed.to_string();
        }
        let Some(map) = body.as_object_mut() else {
            return normalized;
        };
        map.insert("model".to_string(), Value::String(normalized.clone()));
        map.insert(
            "thinking".to_string(),
            serde_json::json!({
                "type": "enabled",
                "budget_tokens": CLAUDECODE_THINKING_BUDGET_TOKENS
            }),
        );
        return normalized;
    }

    trimmed.to_string()
}

pub(super) fn should_expand_claudecode_model_list(
    method: &WreqMethod,
    url: &str,
    body: Option<&Vec<u8>>,
) -> bool {
    *method == WreqMethod::GET
        && body.is_none()
        && (url.contains("/v1/models?") || url.ends_with("/v1/models"))
        && !url.contains("/v1/models/")
}

pub(super) fn extend_model_list_with_thinking_variants(data: &mut Vec<Value>) {
    let existing_ids = data
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<std::collections::BTreeSet<_>>();

    let mut out = Vec::with_capacity(data.len().saturating_mul(3));
    for item in data.iter() {
        out.push(item.clone());

        let Some(id) = item.get("id").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        let id_lower = id.to_ascii_lowercase();
        if id.is_empty()
            || id_lower.ends_with(CLAUDECODE_THINKING_MODEL_SUFFIX)
            || id_lower.ends_with(CLAUDECODE_ADAPTIVE_THINKING_MODEL_SUFFIX)
        {
            continue;
        }

        let thinking_id = format!("{id}{CLAUDECODE_THINKING_MODEL_SUFFIX}");
        let adaptive_thinking_id = format!("{id}{CLAUDECODE_ADAPTIVE_THINKING_MODEL_SUFFIX}");
        for variant_id in [thinking_id, adaptive_thinking_id] {
            if existing_ids.contains(variant_id.as_str()) {
                continue;
            }

            let mut variant_item = item.clone();
            if let Some(obj) = variant_item.as_object_mut() {
                obj.insert("id".to_string(), Value::String(variant_id));
                out.push(variant_item);
            }
        }
    }

    *data = out;
}

pub(super) fn requires_claudecode_sampling_guard(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("opus-4-1")
        || lower.contains("opus-4-5")
        || lower.contains("opus-4-6")
        || lower.contains("sonnet-4-5")
}

pub(super) fn claudecode_credential_update(
    credential_id: i64,
    refreshed: &ClaudeCodeRefreshedToken,
) -> UpstreamCredentialUpdate {
    UpstreamCredentialUpdate::ClaudeCodeTokenRefresh {
        credential_id,
        access_token: Some(refreshed.access_token.clone()),
        refresh_token: Some(refreshed.refresh_token.clone()),
        expires_at_unix_ms: Some(refreshed.expires_at_unix_ms),
        subscription_type: refreshed.subscription_type.clone(),
        rate_limit_tier: refreshed.rate_limit_tier.clone(),
        user_email: refreshed.user_email.clone(),
        cookie: refreshed.cookie.clone(),
        enable_claude_1m_sonnet: None,
        enable_claude_1m_opus: None,
    }
}
