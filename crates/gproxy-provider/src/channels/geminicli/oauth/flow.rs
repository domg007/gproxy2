use super::*;

pub(super) fn generate_state_and_pkce() -> (String, String, String) {
    let mut bytes = rand::random::<[u8; 32]>();
    let state = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);

    bytes = rand::random::<[u8; 32]>();
    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let digest = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

    (state, code_verifier, code_challenge)
}

pub(super) fn build_authorize_url(
    auth_url: &str,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", OAUTH_SCOPE)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("code_challenge_method", "S256")
        .append_pair("code_challenge", code_challenge)
        .append_pair("state", state);
    let query = serializer.finish();
    format!("{}?{}", auth_url.trim_end_matches('/'), query)
}

pub(super) fn resolve_manual_code_and_state(
    query: Option<&str>,
    mode: Option<GeminiCliOAuthMode>,
) -> Result<(String, Option<String>), &'static str> {
    let direct_code =
        parse_query_value(query, "code").or_else(|| parse_query_value(query, "user_code"));
    let direct_state = parse_query_value(query, "state");
    let callback_url = parse_query_value(query, "callback_url");

    match mode {
        Some(GeminiCliOAuthMode::UserCode) => {
            if let Some(code) = direct_code {
                return Ok((code, direct_state));
            }
            return Err("missing user_code or code");
        }
        Some(GeminiCliOAuthMode::AuthorizationCode) => {
            if let Some(callback_url) = callback_url {
                let (code, state) = extract_code_state_from_callback_url(callback_url.as_str());
                if let Some(code) = code {
                    let resolved_state = direct_state.or(state);
                    return Ok((code, resolved_state));
                }
            }
            if let Some(code) = direct_code {
                return Ok((code, direct_state));
            }
            return Err("missing callback_url or code");
        }
        None => {}
    }

    if let Some(code) = direct_code {
        return Ok((code, direct_state));
    }

    if let Some(callback_url) = callback_url {
        let (code, state) = extract_code_state_from_callback_url(callback_url.as_str());
        if let Some(code) = code {
            let resolved_state = direct_state.or(state);
            return Ok((code, resolved_state));
        }
    }

    Err("missing code")
}

pub(super) fn extract_code_state_from_callback_url(
    value: &str,
) -> (Option<String>, Option<String>) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return (None, None);
    }

    let normalized = trimmed.replace("&amp;", "&");

    if let Ok(parsed) = url::Url::parse(normalized.as_str())
        && let Some(query) = parsed.query()
    {
        let (code, state) = extract_code_state_from_query(query);
        if code.is_some() || state.is_some() {
            return (code, state);
        }
    }

    let query = if let Some((_, query)) = normalized.split_once('?') {
        query
    } else {
        normalized.trim_start_matches('?')
    };
    if query.is_empty() {
        return (None, None);
    }
    let query = query.split_once('#').map_or(query, |(value, _)| value);
    extract_code_state_from_query(query)
}

pub(super) fn extract_code_state_from_query(query: &str) -> (Option<String>, Option<String>) {
    let mut code = None;
    let mut state = None;
    for item in query.split('&') {
        if item.is_empty() {
            continue;
        }
        let (raw_name, raw_value) = item.split_once('=').unwrap_or((item, ""));
        let name = percent_decode_component(raw_name);
        let value = percent_decode_component(raw_value);
        if name == "code" {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                code = Some(trimmed.to_string());
            }
            continue;
        }
        if name == "state" {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                state = Some(trimmed.to_string());
            }
        }
    }
    (code, state)
}

pub(super) fn percent_decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_nibble(bytes[index + 1]), hex_nibble(bytes[index + 2]))
        {
            output.push((high << 4) | low);
            index += 3;
            continue;
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(output.as_slice()).into_owned()
}

const fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub(super) fn json_oauth_response(
    status_code: u16,
    body: serde_json::Value,
) -> UpstreamOAuthResponse {
    json_oauth_response_with_meta(status_code, body, None)
}

pub(super) fn json_oauth_response_with_meta(
    status_code: u16,
    body: serde_json::Value,
    request_meta: Option<UpstreamRequestMeta>,
) -> UpstreamOAuthResponse {
    UpstreamOAuthResponse {
        status_code,
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body: serde_json::to_vec(&body).unwrap_or_default(),
        request_meta,
    }
}

pub(super) fn json_oauth_error(status_code: u16, message: &str) -> UpstreamOAuthResponse {
    json_oauth_error_with_meta(status_code, message, None)
}

pub(super) fn json_oauth_error_with_meta(
    status_code: u16,
    message: &str,
    request_meta: Option<UpstreamRequestMeta>,
) -> UpstreamOAuthResponse {
    json_oauth_response_with_meta(status_code, json!({ "error": message }), request_meta)
}

pub(super) fn geminicli_oauth_authorize_url(settings: &ChannelSettings) -> &str {
    settings
        .oauth_authorize_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_AUTH_URL)
}

pub(super) fn geminicli_oauth_token_url(settings: &ChannelSettings) -> &str {
    settings
        .oauth_token_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_TOKEN_URL)
}

pub(super) fn geminicli_oauth_userinfo_url(settings: &ChannelSettings) -> &str {
    settings
        .oauth_userinfo_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(USERINFO_URL)
}
