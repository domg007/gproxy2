use super::stream::random_request_id;
use super::*;

pub(super) fn build_grok_web_payload(
    prompt: &str,
    settings: &GrokSettings,
    model: &GrokResolvedModel,
) -> Result<Vec<u8>, UpstreamError> {
    let mut response_metadata = Map::new();
    response_metadata.insert(
        "requestModelDetails".to_string(),
        json!({ "modelId": model.request_model }),
    );

    let mut payload = Map::new();
    payload.insert(
        "deviceEnvInfo".to_string(),
        json!({
            "darkModeEnabled": false,
            "devicePixelRatio": 2,
            "screenWidth": 2056,
            "screenHeight": 1329,
            "viewportWidth": 2056,
            "viewportHeight": 1083,
        }),
    );
    payload.insert(
        "disableMemory".to_string(),
        Value::Bool(settings.disable_memory),
    );
    payload.insert("disableSearch".to_string(), Value::Bool(false));
    payload.insert(
        "disableSelfHarmShortCircuit".to_string(),
        Value::Bool(false),
    );
    payload.insert("disableTextFollowUps".to_string(), Value::Bool(false));
    payload.insert("enableImageGeneration".to_string(), Value::Bool(false));
    payload.insert("enableImageStreaming".to_string(), Value::Bool(false));
    payload.insert("enableSideBySide".to_string(), Value::Bool(false));
    payload.insert("fileAttachments".to_string(), Value::Array(Vec::new()));
    payload.insert("forceConcise".to_string(), Value::Bool(false));
    payload.insert("forceSideBySide".to_string(), Value::Bool(false));
    payload.insert("imageAttachments".to_string(), Value::Array(Vec::new()));
    payload.insert("imageGenerationCount".to_string(), Value::from(0_u64));
    payload.insert("isAsyncChat".to_string(), Value::Bool(false));
    payload.insert("isReasoning".to_string(), Value::Bool(false));
    payload.insert("message".to_string(), Value::String(prompt.to_string()));
    payload.insert(
        "modelName".to_string(),
        Value::String(model.upstream_model.clone()),
    );
    if let Some(mode) = model
        .upstream_mode
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        payload.insert("modelMode".to_string(), Value::String(mode.to_string()));
    }
    payload.insert(
        "responseMetadata".to_string(),
        Value::Object(response_metadata),
    );
    payload.insert("returnImageBytes".to_string(), Value::Bool(false));
    payload.insert("returnRawGrokInXaiRequest".to_string(), Value::Bool(false));
    payload.insert("sendFinalMetadata".to_string(), Value::Bool(true));
    payload.insert("temporary".to_string(), Value::Bool(settings.temporary));
    payload.insert("toolOverrides".to_string(), Value::Object(Map::new()));

    serde_json::to_vec(&Value::Object(payload))
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

pub(super) fn build_grok_media_post_payload(
    prompt: &str,
    reference_url: Option<&str>,
) -> Result<Vec<u8>, UpstreamError> {
    let mut payload = Map::new();
    if let Some(reference_url) = reference_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        payload.insert(
            "mediaType".to_string(),
            Value::String("MEDIA_POST_TYPE_IMAGE".to_string()),
        );
        payload.insert(
            "mediaUrl".to_string(),
            Value::String(reference_url.to_string()),
        );
    } else {
        payload.insert(
            "mediaType".to_string(),
            Value::String("MEDIA_POST_TYPE_VIDEO".to_string()),
        );
        payload.insert("prompt".to_string(), Value::String(prompt.to_string()));
    }

    serde_json::to_vec(&Value::Object(payload))
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

pub(super) fn build_grok_video_payload(
    prompt: &str,
    parent_post_id: &str,
    aspect_ratio: &str,
    resolution_name: &str,
    video_length: u32,
) -> Result<Vec<u8>, UpstreamError> {
    let mut response_metadata = Map::new();
    response_metadata.insert(
        "requestModelDetails".to_string(),
        json!({ "modelId": "grok-imagine-1.0-video" }),
    );
    response_metadata.insert(
        "modelConfigOverride".to_string(),
        json!({
            "modelMap": {
                "videoGenModelConfig": {
                    "aspectRatio": aspect_ratio,
                    "parentPostId": parent_post_id,
                    "resolutionName": resolution_name,
                    "videoLength": video_length,
                }
            }
        }),
    );

    let mut payload = Map::new();
    payload.insert(
        "deviceEnvInfo".to_string(),
        json!({
            "darkModeEnabled": false,
            "devicePixelRatio": 2,
            "screenWidth": 2056,
            "screenHeight": 1329,
            "viewportWidth": 2056,
            "viewportHeight": 1083,
        }),
    );
    payload.insert("disableMemory".to_string(), Value::Bool(false));
    payload.insert("disableSearch".to_string(), Value::Bool(false));
    payload.insert(
        "disableSelfHarmShortCircuit".to_string(),
        Value::Bool(false),
    );
    payload.insert("disableTextFollowUps".to_string(), Value::Bool(false));
    payload.insert("enableImageGeneration".to_string(), Value::Bool(true));
    payload.insert("enableImageStreaming".to_string(), Value::Bool(true));
    payload.insert("enableSideBySide".to_string(), Value::Bool(true));
    payload.insert("fileAttachments".to_string(), Value::Array(Vec::new()));
    payload.insert("forceConcise".to_string(), Value::Bool(false));
    payload.insert("forceSideBySide".to_string(), Value::Bool(false));
    payload.insert("imageAttachments".to_string(), Value::Array(Vec::new()));
    payload.insert("imageGenerationCount".to_string(), Value::from(2_u64));
    payload.insert("isAsyncChat".to_string(), Value::Bool(false));
    payload.insert("isReasoning".to_string(), Value::Bool(false));
    payload.insert(
        "message".to_string(),
        Value::String(format!("{prompt} --mode=normal").trim().to_string()),
    );
    payload.insert(
        "modelName".to_string(),
        Value::String(super::super::constants::VIDEO_APP_CHAT_MODEL.to_string()),
    );
    payload.insert(
        "responseMetadata".to_string(),
        Value::Object(response_metadata),
    );
    payload.insert("returnImageBytes".to_string(), Value::Bool(false));
    payload.insert("returnRawGrokInXaiRequest".to_string(), Value::Bool(false));
    payload.insert("sendFinalMetadata".to_string(), Value::Bool(true));
    payload.insert("temporary".to_string(), Value::Bool(false));
    payload.insert("toolOverrides".to_string(), json!({ "videoGen": true }));

    serde_json::to_vec(&Value::Object(payload))
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

pub(super) fn build_grok_web_headers(
    resolved_user_agent: Option<&str>,
    extra_cookie_header: Option<&str>,
    sso: &str,
    extra_headers: &[(String, String)],
    base_url: &str,
) -> Result<Vec<(String, String)>, UpstreamError> {
    let user_agent = resolved_user_agent
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_USER_AGENT)
        .to_string();
    let cookie = build_cookie_header(sso, extra_cookie_header)?;
    let (origin, referer) = origin_and_referer(base_url);
    let mut headers = Vec::new();
    merge_extra_headers(&mut headers, extra_headers);
    add_or_replace_header(&mut headers, "accept", "*/*");
    add_or_replace_header(&mut headers, "accept-encoding", "gzip, deflate, br, zstd");
    add_or_replace_header(&mut headers, "accept-language", GROK_ACCEPT_LANGUAGE);
    add_or_replace_header(&mut headers, "baggage", GROK_BAGGAGE);
    add_or_replace_header(&mut headers, "content-type", "application/json");
    add_or_replace_header(&mut headers, "cookie", cookie);
    add_or_replace_header(&mut headers, "origin", origin.as_str());
    add_or_replace_header(&mut headers, "priority", "u=1, i");
    add_or_replace_header(&mut headers, "referer", referer.as_str());
    add_or_replace_header(&mut headers, "sec-fetch-dest", "empty");
    add_or_replace_header(&mut headers, "sec-fetch-mode", "cors");
    add_or_replace_header(&mut headers, "sec-fetch-site", "same-origin");
    add_or_replace_header(&mut headers, "user-agent", user_agent.as_str());
    add_or_replace_header(&mut headers, "x-statsig-id", GROK_STATIC_STATSIG_ID);
    add_or_replace_header(&mut headers, "x-xai-request-id", random_request_id());
    for (name, value) in chromium_client_hints(user_agent.as_str()) {
        add_or_replace_header(&mut headers, name, value);
    }
    Ok(headers)
}

pub(super) fn build_grok_ws_url(base_url: &str, path: &str) -> String {
    let Ok(mut parsed) = Url::parse(base_url) else {
        return format!("wss://grok.com{path}");
    };
    let scheme = match parsed.scheme() {
        "https" => "wss",
        "http" => "ws",
        "wss" => "wss",
        "ws" => "ws",
        _ => "wss",
    };
    let _ = parsed.set_scheme(scheme);
    parsed.set_path(path);
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.to_string()
}

pub(super) fn build_grok_websocket_headers(
    resolved_user_agent: Option<&str>,
    extra_cookie_header: Option<&str>,
    sso: &str,
    extra_headers: &[(String, String)],
    base_url: &str,
) -> Result<Vec<(String, String)>, UpstreamError> {
    let user_agent = resolved_user_agent
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_USER_AGENT)
        .to_string();
    let cookie = build_cookie_header(sso, extra_cookie_header)?;
    let (origin, _) = origin_and_referer(base_url);
    let mut headers = Vec::new();
    merge_extra_headers(&mut headers, extra_headers);
    add_or_replace_header(&mut headers, "accept-language", GROK_ACCEPT_LANGUAGE);
    add_or_replace_header(&mut headers, "cache-control", "no-cache");
    add_or_replace_header(&mut headers, "cookie", cookie);
    add_or_replace_header(&mut headers, "origin", origin.as_str());
    add_or_replace_header(&mut headers, "pragma", "no-cache");
    add_or_replace_header(&mut headers, "user-agent", user_agent.as_str());
    for (name, value) in chromium_client_hints(user_agent.as_str()) {
        add_or_replace_header(&mut headers, name, value);
    }
    Ok(headers)
}

pub(super) fn build_grok_download_headers(
    resolved_user_agent: Option<&str>,
    extra_cookie_header: Option<&str>,
    sso: &str,
    extra_headers: &[(String, String)],
    base_url: &str,
) -> Result<Vec<(String, String)>, UpstreamError> {
    let user_agent = resolved_user_agent
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_USER_AGENT)
        .to_string();
    let cookie = build_cookie_header(sso, extra_cookie_header)?;
    let (origin, referer) = origin_and_referer(base_url);
    let mut headers = Vec::new();
    merge_extra_headers(&mut headers, extra_headers);
    add_or_replace_header(&mut headers, "accept", "*/*");
    add_or_replace_header(&mut headers, "accept-language", GROK_ACCEPT_LANGUAGE);
    add_or_replace_header(&mut headers, "cookie", cookie);
    add_or_replace_header(&mut headers, "origin", origin.as_str());
    add_or_replace_header(&mut headers, "referer", referer.as_str());
    add_or_replace_header(&mut headers, "sec-fetch-dest", "empty");
    add_or_replace_header(&mut headers, "sec-fetch-mode", "cors");
    add_or_replace_header(&mut headers, "sec-fetch-site", "same-origin");
    add_or_replace_header(&mut headers, "user-agent", user_agent.as_str());
    for (name, value) in chromium_client_hints(user_agent.as_str()) {
        add_or_replace_header(&mut headers, name, value);
    }
    Ok(headers)
}

pub(super) fn build_grok_imagine_request_message(
    prompt: &str,
    aspect_ratio: &str,
) -> Result<Vec<u8>, UpstreamError> {
    serde_json::to_vec(&json!({
        "type": "conversation.item.create",
        "timestamp": unix_timestamp_millis(),
        "item": {
            "type": "message",
            "content": [{
                "requestId": random_request_id(),
                "text": prompt,
                "type": "input_text",
                "properties": {
                    "section_count": 0,
                    "is_kids_mode": false,
                    "enable_nsfw": false,
                    "skip_upsampler": false,
                    "is_initial": false,
                    "aspect_ratio": aspect_ratio,
                }
            }]
        }
    }))
    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

fn origin_and_referer(base_url: &str) -> (String, String) {
    let Some(parsed) = Url::parse(base_url).ok() else {
        return (
            "https://grok.com".to_string(),
            "https://grok.com/".to_string(),
        );
    };
    let scheme = parsed.scheme();
    let Some(host) = parsed.host_str() else {
        return (
            "https://grok.com".to_string(),
            "https://grok.com/".to_string(),
        );
    };
    let mut origin = format!("{scheme}://{host}");
    if let Some(port) = parsed.port() {
        origin.push(':');
        origin.push_str(port.to_string().as_str());
    }
    let referer = format!("{origin}/");
    (origin, referer)
}

fn chromium_client_hints(user_agent: &str) -> Vec<(&'static str, String)> {
    let Some(version) = extract_major_version(user_agent, "Chrome/") else {
        return Vec::new();
    };
    let platform = detect_platform(user_agent).unwrap_or("Windows");
    let arch = detect_arch(user_agent).unwrap_or("x86");
    vec![
        (
            "sec-ch-ua",
            format!(
                r#""Google Chrome";v="{version}", "Chromium";v="{version}", "Not(A:Brand";v="24""#
            ),
        ),
        ("sec-ch-ua-mobile", "?0".to_string()),
        ("sec-ch-ua-platform", format!(r#""{platform}""#)),
        ("sec-ch-ua-arch", arch.to_string()),
        ("sec-ch-ua-bitness", "64".to_string()),
        ("sec-ch-ua-model", String::new()),
    ]
}

fn extract_major_version(user_agent: &str, marker: &str) -> Option<String> {
    let start = user_agent.find(marker)? + marker.len();
    let digits = user_agent[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

fn detect_platform(user_agent: &str) -> Option<&'static str> {
    let ua = user_agent.to_ascii_lowercase();
    if ua.contains("windows") {
        Some("Windows")
    } else if ua.contains("mac os x") || ua.contains("macintosh") {
        Some("macOS")
    } else if ua.contains("android") {
        Some("Android")
    } else if ua.contains("iphone") || ua.contains("ipad") {
        Some("iOS")
    } else if ua.contains("linux") {
        Some("Linux")
    } else {
        None
    }
}

fn detect_arch(user_agent: &str) -> Option<&'static str> {
    let ua = user_agent.to_ascii_lowercase();
    if ua.contains("arm") || ua.contains("aarch64") {
        Some("arm")
    } else if ua.contains("x86_64") || ua.contains("win64") || ua.contains("x64") {
        Some("x86")
    } else {
        None
    }
}

fn build_cookie_header(
    sso: &str,
    extra_cookie_header: Option<&str>,
) -> Result<String, UpstreamError> {
    let Some(token) = normalize_sso_material(sso) else {
        return Err(UpstreamError::UnsupportedRequest);
    };
    let mut cookie = format!("sso={token}; sso-rw={token}");
    let extras = extra_cookie_header
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string();
    if !extras.is_empty() {
        cookie.push_str("; ");
        cookie.push_str(extras.as_str());
    }
    Ok(cookie)
}

pub(super) fn normalize_sso_material(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(cookie_value) = cookie_value(raw, "sso") {
        return Some(cookie_value.to_string());
    }
    raw.strip_prefix("sso=")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| Some(raw.to_string()))
}

fn unix_timestamp_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

fn cookie_value<'a>(cookie: &'a str, name: &str) -> Option<&'a str> {
    cookie.split(';').map(str::trim).find_map(|part| {
        let (key, value) = part.split_once('=')?;
        key.eq_ignore_ascii_case(name).then_some(value.trim())
    })
}

#[cfg(test)]
mod tests {
    use super::{cookie_value, normalize_sso_material};

    #[test]
    fn normalize_sso_accepts_cookie_string() {
        assert_eq!(
            normalize_sso_material("sso=abc123; sso-rw=abc123"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn cookie_value_extracts_named_cookie() {
        assert_eq!(
            cookie_value("foo=bar; sso=token; baz=qux", "sso"),
            Some("token")
        );
    }
}
