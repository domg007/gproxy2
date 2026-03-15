use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use dashmap::DashMap;
use rand::Rng as _;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use url::form_urlencoded;
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{
    CLAUDE_API_VERSION, CLAUDE_CODE_UA, CLIENT_ID, DEFAULT_BASE_URL, DEFAULT_CLAUDE_AI_BASE_URL,
    DEFAULT_REDIRECT_URI, OAUTH_BETA, OAUTH_SCOPE, OAUTH_STATE_TTL_MS, TOKEN_REFRESH_SKEW_MS,
    TOKEN_UA,
};
use super::cookie::{TokenResponse, exchange_tokens_with_cookie};
use super::credential::ClaudeCodeCredential;
use crate::channels::ChannelSettings;
use crate::channels::upstream::{
    UpstreamError, UpstreamOAuthCallbackResult, UpstreamOAuthCredential, UpstreamOAuthRequest,
    UpstreamOAuthResponse, UpstreamRequestMeta, tracked_request,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};

#[derive(Debug, Clone)]
struct OAuthState {
    code_verifier: String,
    redirect_uri: String,
    api_base_url: String,
    claude_ai_base_url: String,
    created_at_unix_ms: u64,
}

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct OAuthProfile {
    #[serde(default)]
    account: OAuthProfileAccount,
    #[serde(default)]
    organization: OAuthProfileOrg,
}

#[derive(Debug, Default, Deserialize)]
struct OAuthProfileAccount {
    email: Option<String>,
    #[serde(default)]
    has_claude_max: bool,
    #[serde(default)]
    has_claude_pro: bool,
}

#[derive(Debug, Default, Deserialize)]
struct OAuthProfileOrg {
    rate_limit_tier: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ClaudeCodeAuthMaterial {
    access_token: String,
    refresh_token: String,
    expires_at_unix_ms: u64,
    cookie: Option<String>,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
    user_email: Option<String>,
}

impl ClaudeCodeAuthMaterial {
    pub(crate) fn has_cookie(&self) -> bool {
        self.cookie
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ClaudeCodeRefreshedToken {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) expires_at_unix_ms: u64,
    pub(crate) subscription_type: Option<String>,
    pub(crate) rate_limit_tier: Option<String>,
    pub(crate) user_email: Option<String>,
    pub(crate) cookie: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ClaudeCodeResolvedAccessToken {
    pub(crate) access_token: String,
    pub(crate) refreshed: Option<ClaudeCodeRefreshedToken>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ClaudeCodeTokenRefreshError {
    #[error("invalid claudecode credential: {0}")]
    InvalidCredential(String),
    #[error("transient claudecode token refresh error: {0}")]
    Transient(String),
}

impl ClaudeCodeTokenRefreshError {
    pub(crate) fn as_message(&self) -> String {
        self.to_string()
    }

    pub(crate) fn is_invalid_credential(&self) -> bool {
        matches!(self, Self::InvalidCredential(_))
    }
}

fn oauth_states() -> &'static DashMap<String, OAuthState> {
    static STATES: OnceLock<DashMap<String, OAuthState>> = OnceLock::new();
    STATES.get_or_init(DashMap::new)
}

pub(crate) fn claudecode_access_token_from_credential(
    credential: &ClaudeCodeCredential,
) -> Option<ClaudeCodeAuthMaterial> {
    let access_token = credential.access_token.trim().to_string();
    let refresh_token = credential.refresh_token.trim().to_string();
    let cookie = credential
        .cookie
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    if access_token.is_empty() && refresh_token.is_empty() && cookie.is_none() {
        return None;
    }

    Some(ClaudeCodeAuthMaterial {
        access_token,
        refresh_token,
        expires_at_unix_ms: normalize_expires_at_ms(credential.expires_at),
        cookie,
        subscription_type: (!credential.subscription_type.trim().is_empty())
            .then(|| credential.subscription_type.clone()),
        rate_limit_tier: (!credential.rate_limit_tier.trim().is_empty())
            .then(|| credential.rate_limit_tier.clone()),
        user_email: credential.user_email.clone(),
    })
}

pub(crate) async fn resolve_claudecode_access_token(
    client: &WreqClient,
    cache_key: &str,
    material: &ClaudeCodeAuthMaterial,
    api_base_url: &str,
    claude_ai_base_url: &str,
    now_unix_ms: u64,
    force_refresh: bool,
) -> Result<ClaudeCodeResolvedAccessToken, ClaudeCodeTokenRefreshError> {
    let _ = cache_key;
    if !force_refresh && access_token_valid(material, now_unix_ms) {
        return Ok(ClaudeCodeResolvedAccessToken {
            access_token: material.access_token.clone(),
            refreshed: None,
        });
    }

    let refreshed = refresh_claudecode_access_token(
        client,
        material,
        api_base_url,
        claude_ai_base_url,
        now_unix_ms,
    )
    .await?;
    let resolved = ClaudeCodeResolvedAccessToken {
        access_token: refreshed.access_token.clone(),
        refreshed: Some(refreshed),
    };
    Ok(resolved)
}

pub async fn execute_claudecode_oauth_start(
    _client: &WreqClient,
    settings: &ChannelSettings,
    request: &UpstreamOAuthRequest,
) -> Result<UpstreamOAuthResponse, UpstreamError> {
    let now_unix_ms = current_unix_ms();
    prune_oauth_states(now_unix_ms);

    let redirect_uri = parse_query_value(request.query.as_deref(), "redirect_uri")
        .unwrap_or_else(|| DEFAULT_REDIRECT_URI.to_string());
    let scope = parse_query_value(request.query.as_deref(), "scope")
        .unwrap_or_else(|| OAUTH_SCOPE.to_string());
    let default_claude_ai_base = settings
        .claudecode_ai_base_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_CLAUDE_AI_BASE_URL)
        .to_string();
    let claude_ai_base = parse_query_value(request.query.as_deref(), "claude_ai_base_url")
        .unwrap_or(default_claude_ai_base);
    let api_base =
        parse_query_value(request.query.as_deref(), "api_base_url").unwrap_or_else(|| {
            let base = settings.base_url().trim();
            if base.is_empty() {
                DEFAULT_BASE_URL.to_string()
            } else {
                base.to_string()
            }
        });

    let state_id = generate_oauth_state();
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(code_verifier.as_str());

    oauth_states().insert(
        state_id.clone(),
        OAuthState {
            code_verifier,
            redirect_uri: redirect_uri.clone(),
            api_base_url: api_base,
            claude_ai_base_url: claude_ai_base.clone(),
            created_at_unix_ms: now_unix_ms,
        },
    );

    let auth_url = build_authorize_url(
        claude_ai_base.as_str(),
        redirect_uri.as_str(),
        scope.as_str(),
        code_challenge.as_str(),
        state_id.as_str(),
    );

    Ok(json_oauth_response(
        200,
        json!({
            "auth_url": auth_url,
            "state": state_id,
            "redirect_uri": redirect_uri,
            "mode": "manual",
            "instructions": "Open auth_url, then call /oauth/callback with code/state (or callback_url).",
        }),
    ))
}

pub async fn execute_claudecode_oauth_callback(
    client: &WreqClient,
    _settings: &ChannelSettings,
    request: &UpstreamOAuthRequest,
) -> Result<UpstreamOAuthCallbackResult, UpstreamError> {
    if let Some(error) = parse_query_value(request.query.as_deref(), "error") {
        let detail =
            parse_query_value(request.query.as_deref(), "error_description").unwrap_or(error);
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, detail.as_str()),
            credential: None,
        });
    }

    let now_unix_ms = current_unix_ms();
    prune_oauth_states(now_unix_ms);

    let (code_param, state_param) = resolve_manual_code_and_state(request.query.as_deref());
    let (state_id, oauth_state) = if let Some(state) = state_param.clone() {
        let value = oauth_states()
            .remove(state.as_str())
            .map(|(_, value)| value);
        (state, value)
    } else {
        let states = oauth_states();
        if states.is_empty() {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "missing state"),
                credential: None,
            });
        }
        if states.len() > 1 {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "ambiguous_state"),
                credential: None,
            });
        }
        let Some(entry) = states.iter().next() else {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "missing state"),
                credential: None,
            });
        };
        let key = entry.key().clone();
        let value = states.remove(key.as_str()).map(|(_, value)| value);
        (key, value)
    };

    let Some(oauth_state) = oauth_state else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, "missing state"),
            credential: None,
        });
    };

    let Some(code) = code_param else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, "missing code"),
            credential: None,
        });
    };

    let (mut tokens, oauth_request_meta) = exchange_code_for_tokens(
        client,
        oauth_state.api_base_url.as_str(),
        oauth_state.claude_ai_base_url.as_str(),
        oauth_state.redirect_uri.as_str(),
        oauth_state.code_verifier.as_str(),
        code.as_str(),
        Some(state_id.as_str()),
    )
    .await?;

    let mut user_email = None;
    if (tokens.subscription_type.is_none() || tokens.rate_limit_tier.is_none())
        && let Ok(profile) = fetch_oauth_profile(
            client,
            oauth_state.api_base_url.as_str(),
            tokens.access_token.as_deref().unwrap_or_default(),
        )
        .await
    {
        if tokens.subscription_type.is_none() {
            tokens.subscription_type = profile.subscription_type;
        }
        if tokens.rate_limit_tier.is_none() {
            tokens.rate_limit_tier = profile.rate_limit_tier;
        }
        user_email = profile.email;
    }

    let Some(access_token) = tokens
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
    else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, "missing_access_token"),
            credential: None,
        });
    };

    let Some(refresh_token) = tokens
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
    else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, "missing_refresh_token"),
            credential: None,
        });
    };

    let expires_at_unix_ms = now_unix_ms.saturating_add(tokens.expires_in.unwrap_or(3600) * 1000);
    let subscription_type = tokens.subscription_type.clone();
    let rate_limit_tier = tokens.rate_limit_tier.clone();
    let credential = UpstreamOAuthCredential {
        label: parse_query_value(request.query.as_deref(), "label"),
        credential: ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(
            ClaudeCodeCredential {
                access_token,
                refresh_token,
                expires_at: expires_at_unix_ms.min(i64::MAX as u64) as i64,
                enable_claude_1m_sonnet: Some(true),
                enable_claude_1m_opus: Some(true),
                subscription_type: subscription_type.clone().unwrap_or_default(),
                rate_limit_tier: rate_limit_tier.clone().unwrap_or_default(),
                cookie: None,
                user_email,
            },
        )),
    };

    let response = json_oauth_response(
        200,
        json!({
            "access_token": tokens.access_token,
            "refresh_token": tokens.refresh_token,
            "expires_in": tokens.expires_in,
            "subscriptionType": subscription_type,
            "rateLimitTier": rate_limit_tier,
        }),
    );
    let mut response = response;
    response.request_meta = Some(oauth_request_meta);

    Ok(UpstreamOAuthCallbackResult {
        response,
        credential: Some(credential),
    })
}

pub(crate) async fn refresh_claudecode_access_token(
    client: &WreqClient,
    material: &ClaudeCodeAuthMaterial,
    api_base_url: &str,
    claude_ai_base_url: &str,
    now_unix_ms: u64,
) -> Result<ClaudeCodeRefreshedToken, ClaudeCodeTokenRefreshError> {
    let cookie = material.cookie.as_deref();

    if !material.refresh_token.trim().is_empty() {
        let body = format!(
            "grant_type=refresh_token&client_id={}&refresh_token={}",
            url_encode(CLIENT_ID),
            url_encode(material.refresh_token.as_str()),
        );

        let response = tracked_request(
            client,
            WreqMethod::POST,
            format!("{}/v1/oauth/token", api_base_url.trim_end_matches('/')).as_str(),
        )
        .header("anthropic-version", CLAUDE_API_VERSION)
        .header("anthropic-beta", OAUTH_BETA)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json, text/plain, */*")
        .header("user-agent", TOKEN_UA)
        .body(body)
        .send()
        .await
        .map_err(|err| ClaudeCodeTokenRefreshError::Transient(err.to_string()))?;

        match parse_refreshed_token_response(response, material, now_unix_ms).await {
            Ok(mut token) => {
                fill_missing_refresh_fields_from_profile(client, api_base_url, &mut token).await;
                return Ok(token);
            }
            Err(ClaudeCodeTokenRefreshError::InvalidCredential(_)) if cookie.is_some() => {
                // Align with clewdr behavior: when refresh_token is invalid, try cookie re-exchange.
            }
            Err(err) => return Err(err),
        }
    }

    if let Some(cookie) = cookie {
        let tokens = exchange_tokens_with_cookie(
            client,
            api_base_url,
            claude_ai_base_url,
            DEFAULT_REDIRECT_URI,
            cookie,
        )
        .await
        .map_err(ClaudeCodeTokenRefreshError::Transient)?;

        let access_token = tokens
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ClaudeCodeTokenRefreshError::Transient(
                    "oauth token response missing access_token".to_string(),
                )
            })?
            .to_string();
        let refresh_token = tokens
            .refresh_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ClaudeCodeTokenRefreshError::Transient(
                    "oauth token response missing refresh_token".to_string(),
                )
            })?
            .to_string();

        let mut refreshed = ClaudeCodeRefreshedToken {
            access_token,
            refresh_token,
            expires_at_unix_ms: now_unix_ms
                .saturating_add(tokens.expires_in.unwrap_or(3600) * 1000),
            subscription_type: tokens
                .subscription_type
                .or_else(|| material.subscription_type.clone()),
            rate_limit_tier: tokens
                .rate_limit_tier
                .or_else(|| material.rate_limit_tier.clone()),
            user_email: material.user_email.clone(),
            cookie: material.cookie.clone(),
        };
        fill_missing_refresh_fields_from_profile(client, api_base_url, &mut refreshed).await;
        return Ok(refreshed);
    }

    Err(ClaudeCodeTokenRefreshError::InvalidCredential(
        "missing refresh_token and cookie".to_string(),
    ))
}

async fn parse_refreshed_token_response(
    response: wreq::Response,
    material: &ClaudeCodeAuthMaterial,
    now_unix_ms: u64,
) -> Result<ClaudeCodeRefreshedToken, ClaudeCodeTokenRefreshError> {
    let status = response.status().as_u16();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| ClaudeCodeTokenRefreshError::Transient(err.to_string()))?;

    let parsed = serde_json::from_slice::<TokenResponse>(&bytes).ok();
    if (200..300).contains(&status) {
        let access_token = parsed
            .as_ref()
            .and_then(|value| value.access_token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ClaudeCodeTokenRefreshError::Transient(
                    "oauth token response missing access_token".to_string(),
                )
            })?
            .to_string();

        let refresh_token = parsed
            .as_ref()
            .and_then(|value| value.refresh_token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(material.refresh_token.as_str())
            .to_string();
        let expires_at_unix_ms = now_unix_ms.saturating_add(
            parsed
                .as_ref()
                .and_then(|value| value.expires_in)
                .unwrap_or(3600)
                * 1000,
        );

        return Ok(ClaudeCodeRefreshedToken {
            access_token,
            refresh_token,
            expires_at_unix_ms,
            subscription_type: parsed
                .as_ref()
                .and_then(|value| value.subscription_type.clone())
                .or_else(|| material.subscription_type.clone()),
            rate_limit_tier: parsed
                .as_ref()
                .and_then(|value| value.rate_limit_tier.clone())
                .or_else(|| material.rate_limit_tier.clone()),
            user_email: material.user_email.clone(),
            cookie: material.cookie.clone(),
        });
    }

    let payload_text = String::from_utf8_lossy(&bytes).to_string();
    let error = parsed
        .as_ref()
        .and_then(|value| value.error.as_deref())
        .unwrap_or_default();
    let description = parsed
        .as_ref()
        .and_then(|value| value.error_description.as_deref())
        .unwrap_or_default();
    let message = if error.is_empty() && description.is_empty() {
        format!("oauth token endpoint status {status}: {payload_text}")
    } else {
        format!("oauth token endpoint status {status}: {error} {description}")
    };

    if is_invalid_oauth_credential_failure(status, error, description) {
        Err(ClaudeCodeTokenRefreshError::InvalidCredential(message))
    } else {
        Err(ClaudeCodeTokenRefreshError::Transient(message))
    }
}

fn is_invalid_oauth_credential_failure(status: u16, error: &str, description: &str) -> bool {
    if !matches!(status, 400 | 401 | 403) {
        return false;
    }
    let joined = format!(
        "{} {}",
        error.to_ascii_lowercase(),
        description.to_ascii_lowercase()
    );
    joined.contains("invalid_grant")
        || joined.contains("invalid_client")
        || joined.contains("unauthorized_client")
        || joined.contains("invalid_scope")
        || joined.contains("invalid_token")
}

async fn exchange_code_for_tokens(
    client: &WreqClient,
    api_base_url: &str,
    claude_ai_base_url: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
    state: Option<&str>,
) -> Result<(TokenResponse, UpstreamRequestMeta), UpstreamError> {
    let cleaned_code = code.split('#').next().unwrap_or(code);
    let cleaned_code = cleaned_code.split('&').next().unwrap_or(cleaned_code);

    let mut body = format!(
        "grant_type=authorization_code&client_id={}&code={}&redirect_uri={}&code_verifier={}",
        url_encode(CLIENT_ID),
        url_encode(cleaned_code),
        url_encode(redirect_uri),
        url_encode(code_verifier),
    );
    if let Some(state) = state {
        body.push_str("&state=");
        body.push_str(url_encode(state).as_str());
    }

    let origin = claude_ai_base_url.trim_end_matches('/');
    let token_url = format!("{}/v1/oauth/token", api_base_url.trim_end_matches('/'));
    let sent_headers = vec![
        (
            "anthropic-version".to_string(),
            CLAUDE_API_VERSION.to_string(),
        ),
        ("anthropic-beta".to_string(), OAUTH_BETA.to_string()),
        (
            "content-type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ),
        (
            "accept".to_string(),
            "application/json, text/plain, */*".to_string(),
        ),
        ("user-agent".to_string(), TOKEN_UA.to_string()),
        ("origin".to_string(), origin.to_string()),
        ("referer".to_string(), format!("{origin}/")),
    ];
    let mut req = tracked_request(client, WreqMethod::POST, token_url.as_str());
    for (name, value) in &sent_headers {
        req = req.header(name.as_str(), value.as_str());
    }
    let response = req
        .body(body.clone())
        .send()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let request_meta = crate::channels::upstream::tracked_request_meta(
        WreqMethod::POST.as_str().to_string(),
        token_url.as_str(),
        sent_headers,
        Some(body.into_bytes()),
    );

    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !status.is_success() {
        let text = String::from_utf8_lossy(&bytes);
        return Err(UpstreamError::UpstreamRequest(format!(
            "oauth_token_failed: {status} {text}"
        )));
    }

    let parsed = serde_json::from_slice::<TokenResponse>(&bytes)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    Ok((parsed, request_meta))
}

async fn fetch_oauth_profile(
    client: &WreqClient,
    api_base_url: &str,
    access_token: &str,
) -> Result<OAuthProfileParsed, UpstreamError> {
    let response = tracked_request(
        client,
        WreqMethod::GET,
        format!("{}/api/oauth/profile", api_base_url.trim_end_matches('/')).as_str(),
    )
    .header("authorization", format!("Bearer {access_token}"))
    .header("user-agent", CLAUDE_CODE_UA)
    .header("accept", "application/json")
    .header("anthropic-beta", OAUTH_BETA)
    .send()
    .await
    .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;

    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !status.is_success() {
        let text = String::from_utf8_lossy(&bytes);
        return Err(UpstreamError::UpstreamRequest(format!(
            "oauth_profile_failed: {status} {text}"
        )));
    }

    let payload = serde_json::from_slice::<OAuthProfile>(&bytes)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    Ok(parse_profile(payload))
}

async fn fill_missing_refresh_fields_from_profile(
    client: &WreqClient,
    api_base_url: &str,
    refreshed: &mut ClaudeCodeRefreshedToken,
) {
    if refreshed.subscription_type.is_some()
        && refreshed.rate_limit_tier.is_some()
        && refreshed.user_email.is_some()
    {
        return;
    }

    let Ok(profile) =
        fetch_oauth_profile(client, api_base_url, refreshed.access_token.as_str()).await
    else {
        return;
    };

    if refreshed.subscription_type.is_none() {
        refreshed.subscription_type = profile.subscription_type;
    }
    if refreshed.rate_limit_tier.is_none() {
        refreshed.rate_limit_tier = profile.rate_limit_tier;
    }
    if refreshed.user_email.is_none() {
        refreshed.user_email = profile.email;
    }
}

#[derive(Debug, Default)]
struct OAuthProfileParsed {
    email: Option<String>,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

fn parse_profile(profile: OAuthProfile) -> OAuthProfileParsed {
    let subscription_type = if profile.account.has_claude_max {
        Some("claude_max".to_string())
    } else if profile.account.has_claude_pro {
        Some("claude_pro".to_string())
    } else {
        None
    };

    OAuthProfileParsed {
        email: profile.account.email,
        subscription_type,
        rate_limit_tier: profile.organization.rate_limit_tier,
    }
}

fn resolve_manual_code_and_state(query: Option<&str>) -> (Option<String>, Option<String>) {
    let mut code = parse_query_value(query, "code");
    let mut state = parse_query_value(query, "state");

    if let Some(callback_url) = parse_query_value(query, "callback_url") {
        let (callback_code, callback_state) =
            extract_code_state_from_callback_url(callback_url.as_str());
        if code.is_none() {
            code = callback_code;
        }
        if state.is_none() {
            state = callback_state;
        }
    }

    (
        code.and_then(|value| (!value.trim().is_empty()).then_some(value)),
        state,
    )
}

fn extract_code_state_from_callback_url(callback_url: &str) -> (Option<String>, Option<String>) {
    let raw = callback_url.trim();
    if raw.is_empty() {
        return (None, None);
    }
    let query = raw.split_once('?').map(|(_, query)| query).unwrap_or(raw);
    (
        parse_query_value(Some(query), "code"),
        parse_query_value(Some(query), "state"),
    )
}

fn build_authorize_url(
    claude_ai_base_url: &str,
    redirect_uri: &str,
    scope: &str,
    code_challenge: &str,
    state: &str,
) -> String {
    let mut pairs = vec![
        ("code".to_string(), "true".to_string()),
        ("client_id".to_string(), CLIENT_ID.to_string()),
        ("response_type".to_string(), "code".to_string()),
        ("redirect_uri".to_string(), redirect_uri.to_string()),
        ("scope".to_string(), scope.to_string()),
        ("code_challenge".to_string(), code_challenge.to_string()),
        ("code_challenge_method".to_string(), "S256".to_string()),
        ("state".to_string(), state.to_string()),
    ];

    let query = pairs
        .drain(..)
        .map(|(key, value)| {
            format!(
                "{key}={}",
                form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("&");

    format!(
        "{}/oauth/authorize?{query}",
        claude_ai_base_url.trim_end_matches('/')
    )
}

fn prune_oauth_states(now_unix_ms: u64) {
    let expired = oauth_states()
        .iter()
        .filter_map(|entry| {
            (now_unix_ms.saturating_sub(entry.created_at_unix_ms) > OAUTH_STATE_TTL_MS)
                .then(|| entry.key().clone())
        })
        .collect::<Vec<_>>();

    for key in expired {
        oauth_states().remove(key.as_str());
    }
}

fn json_oauth_response(status_code: u16, body: serde_json::Value) -> UpstreamOAuthResponse {
    UpstreamOAuthResponse {
        status_code,
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body: serde_json::to_vec(&body).unwrap_or_default(),
        request_meta: None,
    }
}

fn json_oauth_error(status_code: u16, message: &str) -> UpstreamOAuthResponse {
    json_oauth_response(status_code, json!({ "error": message }))
}

fn generate_oauth_state() -> String {
    let mut state_bytes = [0u8; 24];
    rand::rng().fill_bytes(&mut state_bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(state_bytes)
}

fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_challenge(code_verifier: &str) -> String {
    let digest = Sha256::digest(code_verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn access_token_valid(material: &ClaudeCodeAuthMaterial, now_unix_ms: u64) -> bool {
    !material.access_token.trim().is_empty()
        && material
            .expires_at_unix_ms
            .saturating_sub(TOKEN_REFRESH_SKEW_MS)
            > now_unix_ms
}

fn normalize_expires_at_ms(expires_at: i64) -> u64 {
    if expires_at <= 0 {
        return 0;
    }
    let value = expires_at as u64;
    if value >= 1_000_000_000_000 {
        value
    } else {
        value.saturating_mul(1000)
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn parse_query_value(query: Option<&str>, key: &str) -> Option<String> {
    let raw = query?.trim().trim_start_matches('?');
    for (name, value) in form_urlencoded::parse(raw.as_bytes()) {
        if name == key {
            return Some(value.into_owned());
        }
    }
    None
}

fn url_encode(value: &str) -> String {
    form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
}
