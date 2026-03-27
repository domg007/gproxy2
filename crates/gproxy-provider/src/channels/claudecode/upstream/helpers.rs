use super::*;
use crate::channels::claudecode::constants::{
    CLAUDE_CODE_BILLING_CCH, CLAUDE_CODE_BILLING_ENTRYPOINT, CLAUDE_CODE_BILLING_HEADER_PREFIX,
    CLAUDE_CODE_BILLING_SALT, CLAUDE_CODE_VERSION,
};
use crate::channels::claudecode::oauth;
use sha2::{Digest as _, Sha256};

pub(super) fn ensure_oauth_beta(headers: &mut Vec<(String, String)>, allow_context_1m: bool) {
    merge_claudecode_beta_headers(headers, &[], allow_context_1m);
}

pub(super) fn merge_claudecode_beta_headers(
    headers: &mut Vec<(String, String)>,
    preferred: &[String],
    allow_context_1m: bool,
) {
    let values = normalized_claudecode_beta_values(
        preferred,
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
    merge_claudecode_beta_headers(headers, &[], false);
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
    allow_context_1m: bool,
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
        if !allow_context_1m && is_context_1m_beta(value) {
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

pub(super) fn apply_claudecode_billing_header_system_block(body: &mut Value) {
    canonicalize_claude_body(body);
    if system_has_claudecode_billing_header(body.get("system")) {
        return;
    }
    let header_text = build_claudecode_billing_header_text(body);
    let Some(map) = body.as_object_mut() else {
        return;
    };

    let header_block = json_text_block(header_text.as_str());
    match map.remove("system") {
        Some(Value::Array(mut blocks)) => {
            blocks.retain(|block| !is_claudecode_billing_header_block(block));
            blocks.insert(0, header_block);
            map.insert("system".to_string(), Value::Array(blocks));
        }
        Some(value) => {
            let mut blocks = vec![header_block];
            if !is_claudecode_billing_header_block(&value) {
                blocks.push(value);
            }
            map.insert("system".to_string(), Value::Array(blocks));
        }
        None => {
            map.insert("system".to_string(), Value::Array(vec![header_block]));
        }
    }
}

pub(super) fn flatten_system_text_before_cache_control(body: &mut Value) {
    canonicalize_claude_body(body);
    let Some(blocks) = body.get_mut("system").and_then(Value::as_array_mut) else {
        return;
    };

    let mut merge_ranges = Vec::new();
    let mut run_start = None;
    let mut run_text = String::new();

    for (index, block) in blocks.iter().enumerate() {
        if is_claudecode_billing_header_block(block) {
            run_start = None;
            run_text.clear();
            continue;
        }

        if block_has_cache_control(block) {
            if let Some(start) = run_start.take()
                && index.saturating_sub(start) > 1
            {
                merge_ranges.push((start, index, std::mem::take(&mut run_text)));
            } else {
                run_text.clear();
            }
            continue;
        }

        if let Some(text) = block_text(block) {
            if run_start.is_none() {
                run_start = Some(index);
            }
            run_text.push_str(text);
            continue;
        }

        run_start = None;
        run_text.clear();
    }

    for (start, end, text) in merge_ranges.into_iter().rev() {
        blocks.splice(start..end, [json_text_block(text.as_str())]);
    }
}

pub(super) fn inject_claudecode_metadata_user_id(
    body: Option<&[u8]>,
    credential_id: i64,
    material: &oauth::ClaudeCodeAuthMaterial,
) -> Option<Vec<u8>> {
    let body = body?;
    let mut body_json = serde_json::from_slice::<Value>(body).ok()?;
    apply_claudecode_metadata_user_id_json(&mut body_json, credential_id, material);
    serde_json::to_vec(&body_json).ok()
}

pub(super) fn apply_claudecode_metadata_user_id_json(
    body: &mut Value,
    credential_id: i64,
    material: &oauth::ClaudeCodeAuthMaterial,
) {
    canonicalize_claude_body(body);
    let session_seed = session_seed_from_body(body)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| credential_id.to_string());
    let Some(map) = body.as_object_mut() else {
        return;
    };

    if map
        .get("metadata")
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("user_id"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return;
    }

    let metadata = map
        .entry("metadata".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(metadata_map) = metadata.as_object_mut() else {
        return;
    };

    let account_uuid = material.account_uuid.clone().unwrap_or_default();
    let device_seed = material
        .account_uuid
        .as_deref()
        .filter(|value: &&str| !value.trim().is_empty())
        .or_else(|| {
            material
                .organization_uuid
                .as_deref()
                .filter(|value: &&str| !value.trim().is_empty())
        })
        .or_else(|| {
            material
                .user_email
                .as_deref()
                .filter(|value: &&str| !value.trim().is_empty())
        })
        .unwrap_or("claudecode");

    metadata_map.insert(
        "user_id".to_string(),
        serde_json::Value::String(
            serde_json::json!({
                "device_id": sha256_hex(format!("gproxy.claudecode.device:{device_seed}").as_str()),
                "account_uuid": account_uuid,
                "session_id": stable_session_uuid(session_seed.as_str()),
            })
            .to_string(),
        ),
    );
}

fn system_has_claudecode_billing_header(system: Option<&Value>) -> bool {
    let Some(system) = system else {
        return false;
    };

    match system {
        Value::Array(blocks) => blocks.iter().any(is_claudecode_billing_header_block),
        value => is_claudecode_billing_header_block(value),
    }
}

fn build_claudecode_billing_header_text(body: &Value) -> String {
    let user_text = first_claudecode_user_text(body);
    let version_hash = claudecode_billing_version_hash(user_text.as_str());
    format!(
        "{} cc_version={}.{}; cc_entrypoint={}; cch={};",
        CLAUDE_CODE_BILLING_HEADER_PREFIX,
        CLAUDE_CODE_VERSION,
        version_hash,
        CLAUDE_CODE_BILLING_ENTRYPOINT,
        CLAUDE_CODE_BILLING_CCH,
    )
}

fn first_claudecode_user_text(body: &Value) -> String {
    body.get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| {
            messages.iter().find_map(|message| {
                let message_map = message.as_object()?;
                if message_map.get("role").and_then(Value::as_str) != Some("user") {
                    return None;
                }
                message_map
                    .get("content")
                    .and_then(first_text_from_claude_content)
            })
        })
        .unwrap_or_default()
}

fn block_has_cache_control(block: &Value) -> bool {
    block
        .as_object()
        .is_some_and(|block_map| block_map.contains_key("cache_control"))
}

fn block_text(block: &Value) -> Option<&str> {
    let block_map = block.as_object()?;
    if block_map.get("type").and_then(Value::as_str) != Some("text") {
        return None;
    }
    block_map.get("text").and_then(Value::as_str)
}

fn first_text_from_claude_content(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => blocks.iter().find_map(first_text_from_claude_block),
        Value::Object(_) => first_text_from_claude_block(content),
        _ => None,
    }
}

fn first_text_from_claude_block(block: &Value) -> Option<String> {
    let block_map = block.as_object()?;
    if block_map.get("type").and_then(Value::as_str) != Some("text") {
        return None;
    }
    block_map
        .get("text")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn session_seed_from_body(body: &Value) -> Option<String> {
    system_session_seed(body).or_else(|| first_message_session_seed(body))
}

fn system_session_seed(body: &Value) -> Option<String> {
    match body.get("system")? {
        Value::String(text) => non_empty_owned(text),
        Value::Array(blocks) => {
            let text = blocks
                .iter()
                .filter_map(first_text_from_claude_block)
                .collect::<Vec<_>>()
                .join("\n");
            non_empty_owned(text.as_str())
        }
        Value::Object(_) => first_text_from_claude_block(body.get("system")?),
        _ => None,
    }
}

fn first_message_session_seed(body: &Value) -> Option<String> {
    let messages = body.get("messages")?.as_array()?;
    let first = messages.first()?.as_object()?;
    first
        .get("content")
        .and_then(first_text_from_claude_content)
        .or_else(|| {
            first
                .get("role")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .and_then(|value| non_empty_owned(value.as_str()))
}

fn non_empty_owned(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn claudecode_billing_version_hash(message_text: &str) -> String {
    let sampled = sampled_js_utf16_positions(message_text, &[4, 7, 20]);
    sha256_hex_prefix(
        format!(
            "{}{}{}",
            CLAUDE_CODE_BILLING_SALT, sampled, CLAUDE_CODE_VERSION
        )
        .as_str(),
        3,
    )
}

fn sampled_js_utf16_positions(text: &str, indices: &[usize]) -> String {
    let utf16 = text.encode_utf16().collect::<Vec<_>>();
    let mut sampled = String::new();
    for index in indices {
        match utf16.get(*index).copied() {
            Some(unit) => sampled.push(js_utf16_unit_char(unit)),
            None => sampled.push('0'),
        }
    }
    sampled
}

fn js_utf16_unit_char(unit: u16) -> char {
    char::from_u32(unit as u32).unwrap_or(char::REPLACEMENT_CHARACTER)
}

fn sha256_hex_prefix(value: &str, len: usize) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let hex = format!("{digest:x}");
    hex[..len.min(hex.len())].to_string()
}

fn sha256_hex(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn stable_session_uuid(seed: &str) -> String {
    let digest = Sha256::digest(format!("gproxy.claudecode.session:{seed}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

fn is_claudecode_billing_header_block(block: &Value) -> bool {
    block
        .as_object()
        .and_then(|block_map| block_map.get("text"))
        .and_then(Value::as_str)
        .map(str::trim_start)
        .is_some_and(|text| text.starts_with(CLAUDE_CODE_BILLING_HEADER_PREFIX))
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
        account_uuid: None,
        organization_uuid: None,
        cookie: None,
        enable_claude_1m_sonnet: disable_sonnet,
        enable_claude_1m_opus: disable_opus,
    });
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
        account_uuid: refreshed.account_uuid.clone(),
        organization_uuid: refreshed.organization_uuid.clone(),
        cookie: refreshed.cookie.clone(),
        enable_claude_1m_sonnet: None,
        enable_claude_1m_opus: None,
    }
}
