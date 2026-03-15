use super::*;

pub(super) fn ensure_oauth_beta(headers: &mut Vec<(String, String)>) {
    merge_claudecode_beta_headers(headers, &[]);
}

pub(super) fn merge_claudecode_beta_headers(
    headers: &mut Vec<(String, String)>,
    preferred: &[String],
) {
    let values = normalized_claudecode_beta_values(
        preferred,
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
            .map(|(_, value)| parse_anthropic_beta_values(value))
            .unwrap_or_default(),
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
    preferred: &[String],
    values: Vec<String>,
) -> Vec<String> {
    let mut merged = Vec::new();

    for raw in preferred
        .iter()
        .map(String::as_str)
        .chain(values.iter().map(String::as_str))
    {
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        if !merged
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(value))
        {
            merged.push(value.to_string());
        }
    }

    if !merged
        .iter()
        .any(|value| value.eq_ignore_ascii_case(OAUTH_BETA))
    {
        merged.push(OAUTH_BETA.to_string());
    }

    merged
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

pub(super) fn normalize_claudecode_sampling(body: &mut Value) {
    let Some(map) = body.as_object_mut() else {
        return;
    };

    let has_temperature = map.get("temperature").and_then(Value::as_f64).is_some();
    let has_top_p = map.get("top_p").and_then(Value::as_f64).is_some();
    if has_temperature && has_top_p {
        map.remove("top_p");
    }
}

pub(super) fn normalize_claudecode_unsupported_fields(body: &mut Value) {
    let Some(map) = body.as_object_mut() else {
        return;
    };

    // Anthropic v1/messages on this upstream path currently rejects this field.
    // map.remove("context_management");
    map.remove("speed");
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
    }
}
