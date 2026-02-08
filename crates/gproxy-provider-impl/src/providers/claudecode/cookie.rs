use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};
use wreq::header::{
    ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, CONTENT_TYPE, COOKIE, HeaderMap, HeaderValue, ORIGIN,
    REFERER, USER_AGENT,
};

use super::*;

static SESSION_TOKEN_CACHE: OnceLock<Mutex<HashMap<String, CachedTokens>>> = OnceLock::new();
static SESSION_COOKIE_KEEPALIVE_CACHE: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();
const SESSION_COOKIE_KEEPALIVE_INTERVAL_SECS: i64 = 5 * 60;

#[derive(Debug, Clone)]
pub(super) struct CachedTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Option<i64>,
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
}

pub(super) fn ensure_session_tokens_full(
    config: &ProviderConfig,
    session_key: &str,
) -> ProviderResult<CachedTokens> {
    keepalive_session_cookie_if_due(config, session_key);
    if let Some(cached) = get_cached_session_tokens(session_key) {
        return Ok(cached);
    }
    let tokens = oauth_with_session_key(config, session_key)?;
    let refresh_token = tokens
        .refresh_token
        .clone()
        .ok_or(ProviderError::MissingCredentialField("refresh_token"))?;
    let cached = CachedTokens {
        access_token: tokens.access_token.clone(),
        refresh_token,
        expires_at: tokens.expires_in.map(|v| v + chrono_now()),
        subscription_type: tokens.subscription_type.clone(),
        rate_limit_tier: tokens.rate_limit_tier.clone(),
    };
    cache_session_tokens(session_key, &cached);
    Ok(cached)
}

pub(super) fn get_cached_session_tokens_any(session_key: &str) -> Option<CachedTokens> {
    let guard = session_cache().lock().ok()?;
    guard.get(session_key).cloned()
}

pub(super) fn cache_session_tokens(session_key: &str, tokens: &CachedTokens) {
    let mut guard = match session_cache().lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    guard.insert(session_key.to_string(), tokens.clone());
}

pub(super) fn clear_session_cache(session_key: &str) {
    if let Ok(mut guard) = session_cache().lock() {
        guard.remove(session_key);
    }
    if let Ok(mut guard) = session_cookie_keepalive_cache().lock() {
        guard.remove(session_key);
    }
}

fn get_cached_session_tokens(session_key: &str) -> Option<CachedTokens> {
    let guard = session_cache().lock().ok()?;
    let cached = guard.get(session_key)?;
    if is_expired(cached.expires_at) {
        return None;
    }
    Some(cached.clone())
}

fn session_cache() -> &'static Mutex<HashMap<String, CachedTokens>> {
    SESSION_TOKEN_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn session_cookie_keepalive_cache() -> &'static Mutex<HashMap<String, i64>> {
    SESSION_COOKIE_KEEPALIVE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn keepalive_session_cookie_if_due(config: &ProviderConfig, session_key: &str) {
    let now = chrono_now();
    if !session_cookie_keepalive_due(session_key, now) {
        return;
    }

    // Keep sessionKey cookie warm on Claude side. Failure is best-effort and
    // should not block normal request flow.
    let Ok(claude_ai_base) = claudecode_ai_base_url(config).map(|value| value.trim_end_matches('/').to_string()) else {
        return;
    };
    let result: ProviderResult<()> = crate::providers::oauth_common::block_on(async move {
        let client = wreq::Client::builder()
            .build()
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        fetch_org_info(&client, session_key, &claude_ai_base).await?;
        Ok(())
    });
    if result.is_ok() {
        mark_session_cookie_keepalive(session_key, now);
    }
}

fn session_cookie_keepalive_due(session_key: &str, now: i64) -> bool {
    let guard = match session_cookie_keepalive_cache().lock() {
        Ok(value) => value,
        Err(_) => return false,
    };
    match guard.get(session_key).copied() {
        Some(next_at) => now >= next_at,
        None => true,
    }
}

fn mark_session_cookie_keepalive(session_key: &str, now: i64) {
    if let Ok(mut guard) = session_cookie_keepalive_cache().lock() {
        guard.insert(
            session_key.to_string(),
            now.saturating_add(SESSION_COOKIE_KEEPALIVE_INTERVAL_SECS),
        );
    }
}

fn is_expired(expires_at: Option<i64>) -> bool {
    let Some(expires_at) = expires_at else {
        return false;
    };
    chrono_now() >= expires_at.saturating_sub(60)
}

fn oauth_with_session_key(
    config: &ProviderConfig,
    session_key: &str,
) -> ProviderResult<TokenResponse> {
    crate::providers::oauth_common::block_on(async move {
        let api_base = claudecode_api_base_url(config)?.trim_end_matches('/');
        let claude_ai_base = claudecode_ai_base_url(config)?.trim_end_matches('/');
        let redirect_uri = claudecode_oauth_redirect_uri(config)?;
        let client = wreq::Client::builder()
            .build()
            .map_err(|err| ProviderError::Other(err.to_string()))?;

        let org = fetch_org_info(&client, session_key, claude_ai_base).await?;
        let (code, code_verifier, _scope, state) = authorize_with_cookie(
            &client,
            session_key,
            claude_ai_base,
            api_base,
            &redirect_uri,
            &org,
        )
        .await?;
        let cleaned_code = code.split('#').next().unwrap_or(&code);
        let cleaned_code = cleaned_code.split('&').next().unwrap_or(cleaned_code);
        let mut body = format!(
            "grant_type=authorization_code&client_id={}&code={}&redirect_uri={}&code_verifier={}",
            urlencoding::encode(CLIENT_ID),
            urlencoding::encode(cleaned_code),
            urlencoding::encode(&redirect_uri),
            urlencoding::encode(&code_verifier),
        );
        body.push_str("&state=");
        body.push_str(&urlencoding::encode(&state));
        let origin = claude_ai_base.trim_end_matches('/');
        let resp = client
            .post(format!("{api_base}/v1/oauth/token"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", TOKEN_UA)
            .header("accept", "application/json, text/plain, */*")
            .header("origin", origin)
            .header("referer", format!("{origin}/"))
            .body(body)
            .send()
            .await
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes);
            return Err(ProviderError::Other(format!(
                "oauth_token_failed: {status} {text}"
            )));
        }
        let tokens = serde_json::from_slice::<TokenResponse>(&bytes)
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        Ok(tokens)
    })
}

struct OrgInfo {
    uuid: String,
}

async fn fetch_org_info(
    client: &wreq::Client,
    session_key: &str,
    claude_ai_base: &str,
) -> ProviderResult<OrgInfo> {
    let url = format!("{claude_ai_base}/api/organizations");
    let headers = build_cookie_headers(session_key, claude_ai_base)?;
    if let Ok(org) = fetch_org_info_from_bootstrap(client, session_key, claude_ai_base).await {
        return Ok(org);
    }
    fetch_org_info_from_organizations(client, &url, headers).await
}

async fn fetch_org_info_from_bootstrap(
    client: &wreq::Client,
    session_key: &str,
    claude_ai_base: &str,
) -> ProviderResult<OrgInfo> {
    let url = format!("{claude_ai_base}/api/bootstrap");
    let headers = build_cookie_headers(session_key, claude_ai_base)?;
    let resp = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let status = resp.status();
    let body = resp
        .bytes()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    if !status.is_success() {
        return Err(ProviderError::Other(format!(
            "sessionkey_org_bootstrap_failed: {status}"
        )));
    }
    let payload = serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let memberships = payload
        .get("account")
        .and_then(|v| v.get("memberships"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            ProviderError::Other("invalid bootstrap memberships response".to_string())
        })?;
    for item in memberships {
        let org = item
            .get("organization")
            .ok_or_else(|| ProviderError::Other("invalid bootstrap organization".to_string()))?;
        let caps = org
            .get("capabilities")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !caps.contains(&"chat") {
            continue;
        }
        let uuid = org
            .get("uuid")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        if let Some(uuid) = uuid {
            return Ok(OrgInfo { uuid });
        }
    }
    Err(ProviderError::Other(
        "no bootstrap organization with chat capability".to_string(),
    ))
}

async fn fetch_org_info_from_organizations(
    client: &wreq::Client,
    url: &str,
    headers: HeaderMap,
) -> ProviderResult<OrgInfo> {
    let resp = client
        .get(url)
        .headers(headers)
        .send()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let status = resp.status();
    let body = resp
        .bytes()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    if !status.is_success() {
        return Err(ProviderError::Other(format!(
            "sessionkey_org_failed: {status}"
        )));
    }
    let payload = serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let list = payload
        .as_array()
        .ok_or_else(|| ProviderError::Other("invalid org list response".to_string()))?;
    for org in list {
        let caps = org
            .get("capabilities")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !caps.contains(&"chat") {
            continue;
        }
        let uuid = org
            .get("uuid")
            .or_else(|| org.get("id"))
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        if let Some(uuid) = uuid {
            return Ok(OrgInfo { uuid });
        }
    }
    Err(ProviderError::Other(
        "no organization with chat capability".to_string(),
    ))
}

async fn authorize_with_cookie(
    client: &wreq::Client,
    session_key: &str,
    claude_ai_base: &str,
    api_base: &str,
    oauth_redirect_uri: &str,
    org: &OrgInfo,
) -> ProviderResult<(String, String, String, String)> {
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);
    let state = generate_state();
    let scope = OAUTH_SCOPE.to_string();
    let url = format!("{api_base}/v1/oauth/{}/authorize", org.uuid);
    let payload = serde_json::json!({
        "response_type": "code",
        "client_id": CLIENT_ID,
        "organization_uuid": org.uuid,
        "redirect_uri": oauth_redirect_uri,
        "scope": scope,
        "state": state,
        "code_challenge": code_challenge,
        "code_challenge_method": "S256",
    });
    let mut headers = build_cookie_headers(session_key, claude_ai_base)?;
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let body = serde_json::to_vec(&payload).map_err(|err| ProviderError::Other(err.to_string()))?;
    let resp = client
        .post(&url)
        .headers(headers)
        .body(body)
        .send()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let status = resp.status();
    let bytes = resp
        .bytes()
        .await
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    if !status.is_success() {
        let text = String::from_utf8_lossy(&bytes);
        return Err(ProviderError::Other(format!(
            "sessionkey_authorize_failed: scope={} status={} body={}",
            OAUTH_SCOPE,
            status,
            text.chars().take(300).collect::<String>()
        )));
    }
    let payload = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let redirect_uri = payload
        .get("redirect_uri")
        .and_then(|value| value.as_str())
        .ok_or_else(|| ProviderError::Other("missing redirect_uri".to_string()))?;
    let code = extract_query_value(redirect_uri, "code")
        .ok_or_else(|| ProviderError::Other("missing code in redirect_uri".to_string()))?;
    Ok((code, code_verifier, scope, state))
}

fn build_cookie_headers(session_key: &str, claude_ai_base: &str) -> ProviderResult<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(
        COOKIE,
        HeaderValue::from_str(&format!("sessionKey={session_key}"))
            .map_err(|err| ProviderError::Other(err.to_string()))?,
    );
    let origin = claude_ai_base.trim_end_matches('/');
    headers.insert(
        ORIGIN,
        HeaderValue::from_str(origin).map_err(|err| ProviderError::Other(err.to_string()))?,
    );
    headers.insert(
        REFERER,
        HeaderValue::from_str(&format!("{origin}/new"))
            .map_err(|err| ProviderError::Other(err.to_string()))?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static(COOKIE_UA));
    Ok(headers)
}

fn generate_state() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let digest = hasher.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn extract_query_value(url: &str, key: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut iter = pair.splitn(2, '=');
        let name = iter.next()?;
        let value = iter.next().unwrap_or("");
        if name == key {
            return urlencoding::decode(value).ok().map(|v| v.to_string());
        }
    }
    None
}
