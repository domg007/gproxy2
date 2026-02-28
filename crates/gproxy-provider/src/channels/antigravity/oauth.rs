use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use dashmap::DashMap;
use rand::RngExt as _;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use url::form_urlencoded;
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{
    ANTIGRAVITY_USER_AGENT, CLIENT_ID, CLIENT_SECRET, DEFAULT_AUTH_URL, DEFAULT_BASE_URL,
    DEFAULT_REDIRECT_URI, DEFAULT_TOKEN_URL, OAUTH_SCOPE, OAUTH_STATE_TTL_MS,
    TOKEN_REFRESH_SKEW_MS, USERINFO_URL,
};
use super::credential::AntigravityCredential;
use crate::channels::ChannelSettings;
use crate::channels::upstream::{
    UpstreamError, UpstreamOAuthCallbackResult, UpstreamOAuthCredential, UpstreamOAuthRequest,
    UpstreamOAuthResponse, UpstreamRequestMeta, tracked_send_request,
};
use crate::channels::utils::parse_query_value;
use crate::channels::{BuiltinChannelCredential, ChannelCredential};

#[derive(Debug, Clone)]
struct OAuthState {
    code_verifier: String,
    redirect_uri: String,
    project_id: Option<String>,
    created_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
struct CachedAntigravityToken {
    access_token: String,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct AntigravityAuthMaterial {
    access_token: String,
    refresh_token: String,
    expires_at_unix_ms: u64,
    pub(crate) project_id: String,
    client_id: String,
    client_secret: String,
    user_email: Option<String>,
}

impl AntigravityAuthMaterial {
    fn access_token_valid(&self, now_unix_ms: u64) -> bool {
        !self.access_token.trim().is_empty()
            && self
                .expires_at_unix_ms
                .saturating_sub(TOKEN_REFRESH_SKEW_MS)
                > now_unix_ms
    }

    fn can_refresh(&self) -> bool {
        !self.refresh_token.trim().is_empty()
            && !self.client_id.trim().is_empty()
            && !self.client_secret.trim().is_empty()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AntigravityRefreshedToken {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) expires_at_unix_ms: u64,
    pub(crate) user_email: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AntigravityResolvedAccessToken {
    pub(crate) access_token: String,
    pub(crate) refreshed: Option<AntigravityRefreshedToken>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AntigravityTokenRefreshError {
    #[error("invalid antigravity credential: {0}")]
    InvalidCredential(String),
    #[error("transient antigravity token refresh error: {0}")]
    Transient(String),
}

impl AntigravityTokenRefreshError {
    pub(crate) fn as_message(&self) -> String {
        self.to_string()
    }

    pub(crate) fn is_invalid_credential(&self) -> bool {
        matches!(self, Self::InvalidCredential(_))
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

fn oauth_states() -> &'static DashMap<String, OAuthState> {
    static STATES: OnceLock<DashMap<String, OAuthState>> = OnceLock::new();
    STATES.get_or_init(DashMap::new)
}

fn antigravity_token_cache() -> &'static DashMap<String, CachedAntigravityToken> {
    static CACHE: OnceLock<DashMap<String, CachedAntigravityToken>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

pub(crate) fn antigravity_auth_material_from_credential(
    value: &AntigravityCredential,
) -> Option<AntigravityAuthMaterial> {
    let material = AntigravityAuthMaterial {
        access_token: value.access_token.trim().to_string(),
        refresh_token: value.refresh_token.trim().to_string(),
        expires_at_unix_ms: normalize_expires_at_ms(value.expires_at),
        project_id: value.project_id.trim().to_string(),
        client_id: value.client_id.trim().to_string(),
        client_secret: value.client_secret.trim().to_string(),
        user_email: value.user_email.clone(),
    };

    if material.access_token.is_empty()
        && material.refresh_token.is_empty()
        && material.client_id.is_empty()
        && material.client_secret.is_empty()
    {
        None
    } else {
        Some(material)
    }
}

pub async fn ensure_antigravity_project_id(
    client: &WreqClient,
    settings: &ChannelSettings,
    credential: &mut AntigravityCredential,
) -> Result<(), UpstreamError> {
    if !credential.project_id.trim().is_empty() {
        return Ok(());
    }

    let now_unix_ms = current_unix_ms();
    let Some(material) = antigravity_auth_material_from_credential(credential) else {
        return Err(UpstreamError::SerializeRequest(
            "invalid antigravity credential: missing auth material".to_string(),
        ));
    };

    let resolved = resolve_antigravity_access_token(
        client,
        settings,
        "antigravity::project-detect",
        &material,
        now_unix_ms,
        false,
    )
    .await
    .map_err(|err| UpstreamError::UpstreamRequest(err.as_message()))?;

    if let Some(refreshed) = resolved.refreshed.as_ref() {
        credential.access_token = refreshed.access_token.clone();
        credential.refresh_token = refreshed.refresh_token.clone();
        credential.expires_at = refreshed.expires_at_unix_ms.min(i64::MAX as u64) as i64;
        if credential
            .user_email
            .as_deref()
            .map(str::trim)
            .is_none_or(|value| value.is_empty())
        {
            credential.user_email = refreshed.user_email.clone();
        }
    }

    let base_url = if settings.base_url().trim().is_empty() {
        DEFAULT_BASE_URL
    } else {
        settings.base_url()
    };
    let project_id = detect_project_id(client, resolved.access_token.as_str(), base_url).await?;
    let project_id = project_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            UpstreamError::SerializeRequest("missing project_id (auto-detect failed)".to_string())
        })?;
    credential.project_id = project_id;
    Ok(())
}

pub(crate) async fn resolve_antigravity_access_token(
    client: &WreqClient,
    settings: &ChannelSettings,
    cache_key: &str,
    material: &AntigravityAuthMaterial,
    now_unix_ms: u64,
    force_refresh: bool,
) -> Result<AntigravityResolvedAccessToken, AntigravityTokenRefreshError> {
    if !force_refresh {
        if let Some(cached) = antigravity_token_cache().get(cache_key).filter(|item| {
            item.expires_at_unix_ms
                .saturating_sub(TOKEN_REFRESH_SKEW_MS)
                > now_unix_ms
        }) {
            return Ok(AntigravityResolvedAccessToken {
                access_token: cached.access_token.clone(),
                refreshed: None,
            });
        }

        if material.access_token_valid(now_unix_ms) {
            antigravity_token_cache().insert(
                cache_key.to_string(),
                CachedAntigravityToken {
                    access_token: material.access_token.clone(),
                    expires_at_unix_ms: material.expires_at_unix_ms,
                },
            );
            return Ok(AntigravityResolvedAccessToken {
                access_token: material.access_token.clone(),
                refreshed: None,
            });
        }
    }

    let refreshed = refresh_access_token(client, settings, material, now_unix_ms).await?;
    antigravity_token_cache().insert(
        cache_key.to_string(),
        CachedAntigravityToken {
            access_token: refreshed.access_token.clone(),
            expires_at_unix_ms: refreshed.expires_at_unix_ms,
        },
    );
    Ok(AntigravityResolvedAccessToken {
        access_token: refreshed.access_token.clone(),
        refreshed: Some(refreshed),
    })
}

pub async fn execute_antigravity_oauth_start(
    _client: &WreqClient,
    settings: &ChannelSettings,
    request: &UpstreamOAuthRequest,
) -> Result<UpstreamOAuthResponse, UpstreamError> {
    let now_unix_ms = current_unix_ms();
    prune_oauth_states(now_unix_ms);

    let redirect_uri = parse_query_value(request.query.as_deref(), "redirect_uri")
        .unwrap_or_else(|| DEFAULT_REDIRECT_URI.to_string());
    let project_id = parse_query_value(request.query.as_deref(), "project_id")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let (state, code_verifier, code_challenge) = generate_state_and_pkce();
    let auth_url = build_authorize_url(
        antigravity_oauth_authorize_url(settings),
        redirect_uri.as_str(),
        state.as_str(),
        code_challenge.as_str(),
    );

    oauth_states().insert(
        state.clone(),
        OAuthState {
            code_verifier,
            redirect_uri: redirect_uri.clone(),
            project_id,
            created_at_unix_ms: now_unix_ms,
        },
    );

    Ok(json_oauth_response(
        200,
        json!({
            "auth_url": auth_url,
            "state": state,
            "redirect_uri": redirect_uri,
            "mode": "manual",
            "instructions": "Open auth_url, then call /oauth/callback with code/state (or callback_url).",
        }),
    ))
}

pub async fn execute_antigravity_oauth_callback(
    client: &WreqClient,
    settings: &ChannelSettings,
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

    let (code, state_param) = resolve_manual_code_and_state(request.query.as_deref())?;
    let (oauth_state, ambiguous_state) = if let Some(state) = state_param.as_deref() {
        (oauth_states().remove(state).map(|(_, value)| value), false)
    } else if oauth_states().is_empty() {
        (None, false)
    } else if oauth_states().len() == 1 {
        let key = oauth_states()
            .iter()
            .next()
            .map(|entry| entry.key().clone());
        let value = key.and_then(|state_id| {
            oauth_states()
                .remove(state_id.as_str())
                .map(|(_, value)| value)
        });
        (value, false)
    } else {
        (None, true)
    };

    if ambiguous_state {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, "ambiguous_state"),
            credential: None,
        });
    }

    let Some(oauth_state) = oauth_state else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, "missing state"),
            credential: None,
        });
    };

    let (tokens, token_request_meta) = exchange_code_for_tokens(
        client,
        antigravity_oauth_token_url(settings),
        code.as_str(),
        oauth_state.redirect_uri.as_str(),
        oauth_state.code_verifier.as_str(),
    )
    .await?;

    let Some(access_token) = tokens
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
    else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error_with_meta(
                400,
                "missing_access_token",
                Some(token_request_meta.clone()),
            ),
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
            response: json_oauth_error_with_meta(
                400,
                "missing_refresh_token",
                Some(token_request_meta.clone()),
            ),
            credential: None,
        });
    };

    let base_url = if settings.base_url().trim().is_empty() {
        DEFAULT_BASE_URL
    } else {
        settings.base_url()
    };
    let detected_project_id = detect_project_id(client, access_token.as_str(), base_url)
        .await
        .ok()
        .flatten();
    let project_id = oauth_state
        .project_id
        .or_else(|| parse_query_value(request.query.as_deref(), "project_id"))
        .filter(|value| !value.trim().is_empty())
        .or(detected_project_id);
    let Some(project_id) = project_id else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error_with_meta(
                400,
                "missing project_id (auto-detect failed)",
                Some(token_request_meta.clone()),
            ),
            credential: None,
        });
    };

    let user_email = fetch_user_email(
        client,
        access_token.as_str(),
        antigravity_oauth_userinfo_url(settings),
    )
    .await
    .ok()
    .flatten();
    let expires_at_unix_ms =
        now_unix_ms.saturating_add(tokens.expires_in.unwrap_or(3600).saturating_mul(1000));
    let credential = UpstreamOAuthCredential {
        label: Some(format!("antigravity:{project_id}")),
        credential: ChannelCredential::Builtin(BuiltinChannelCredential::Antigravity(
            AntigravityCredential {
                access_token: access_token.clone(),
                refresh_token: refresh_token.clone(),
                expires_at: expires_at_unix_ms.min(i64::MAX as u64) as i64,
                project_id: project_id.clone(),
                client_id: CLIENT_ID.to_string(),
                client_secret: CLIENT_SECRET.to_string(),
                user_email: user_email.clone(),
            },
        )),
    };

    Ok(UpstreamOAuthCallbackResult {
        response: json_oauth_response_with_meta(
            200,
            json!({
                "access_token": access_token,
                "refresh_token": refresh_token,
                "project_id": project_id,
                "user_email": user_email,
                "expires_at_unix_ms": expires_at_unix_ms,
            }),
            Some(token_request_meta),
        ),
        credential: Some(credential),
    })
}

async fn refresh_access_token(
    client: &WreqClient,
    settings: &ChannelSettings,
    material: &AntigravityAuthMaterial,
    now_unix_ms: u64,
) -> Result<AntigravityRefreshedToken, AntigravityTokenRefreshError> {
    if !material.can_refresh() {
        return Err(AntigravityTokenRefreshError::InvalidCredential(
            "missing refresh_token/client_id/client_secret".to_string(),
        ));
    }

    let body = {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        serializer
            .append_pair("refresh_token", material.refresh_token.as_str())
            .append_pair("client_id", material.client_id.as_str())
            .append_pair("client_secret", material.client_secret.as_str())
            .append_pair("grant_type", "refresh_token");
        serializer.finish()
    };
    let (token, _request_meta) =
        send_token_request(client, antigravity_oauth_token_url(settings), body.as_str())
            .await
            .map_err(|err| AntigravityTokenRefreshError::Transient(err.to_string()))?;

    let access_token = token
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AntigravityTokenRefreshError::Transient(
                "oauth token response missing access_token".to_string(),
            )
        })?
        .to_string();

    let refresh_token = token
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| material.refresh_token.clone());
    let expires_at_unix_ms =
        now_unix_ms.saturating_add(token.expires_in.unwrap_or(3600).saturating_mul(1000));
    let user_email = if material
        .user_email
        .as_deref()
        .map(str::trim)
        .is_none_or(|value| value.is_empty())
    {
        fetch_user_email(
            client,
            access_token.as_str(),
            antigravity_oauth_userinfo_url(settings),
        )
        .await
        .ok()
        .flatten()
    } else {
        None
    };

    Ok(AntigravityRefreshedToken {
        access_token,
        refresh_token,
        expires_at_unix_ms,
        user_email,
    })
}

async fn exchange_code_for_tokens(
    client: &WreqClient,
    token_url: &str,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<(TokenResponse, UpstreamRequestMeta), UpstreamError> {
    let body = {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        serializer
            .append_pair("code", code)
            .append_pair("client_id", CLIENT_ID)
            .append_pair("client_secret", CLIENT_SECRET)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("code_verifier", code_verifier)
            .append_pair("grant_type", "authorization_code");
        serializer.finish()
    };
    send_token_request(client, token_url, body.as_str()).await
}

async fn send_token_request(
    client: &WreqClient,
    token_url: &str,
    body: &str,
) -> Result<(TokenResponse, UpstreamRequestMeta), UpstreamError> {
    let headers = vec![(
        "content-type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    )];
    let (response, request_meta) = tracked_send_request(
        client,
        WreqMethod::POST,
        token_url,
        headers,
        Some(body.as_bytes().to_vec()),
    )
    .await
    .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let status_code = response.status().as_u16();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let token = serde_json::from_slice::<TokenResponse>(&bytes)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    if !(200..300).contains(&status_code) {
        let error = token.error.as_deref().unwrap_or_default();
        let description = token.error_description.as_deref().unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        let message = if error.is_empty() && description.is_empty() {
            format!("oauth_token_failed: {status_code} {text}")
        } else {
            format!("oauth_token_failed: {status_code} {error} {description}")
        };
        return Err(UpstreamError::UpstreamRequest(message));
    }
    Ok((token, request_meta))
}

async fn detect_project_id(
    client: &WreqClient,
    access_token: &str,
    base_url: &str,
) -> Result<Option<String>, UpstreamError> {
    if let Some(project_id) = try_load_code_assist(client, access_token, base_url).await? {
        return Ok(Some(project_id));
    }
    try_onboard_user(client, access_token, base_url).await
}

async fn try_load_code_assist(
    client: &WreqClient,
    access_token: &str,
    base_url: &str,
) -> Result<Option<String>, UpstreamError> {
    let payload = call_code_assist(client, access_token, base_url).await?;
    if payload.get("currentTier").is_none()
        || payload
            .get("currentTier")
            .map(|value| value.is_null())
            .unwrap_or(true)
    {
        return Ok(None);
    }
    Ok(payload
        .get("cloudaicompanionProject")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned))
}

async fn try_onboard_user(
    client: &WreqClient,
    access_token: &str,
    base_url: &str,
) -> Result<Option<String>, UpstreamError> {
    let tier_id = get_onboard_tier(client, access_token, base_url).await?;
    let body = json!({
        "tierId": tier_id,
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });
    let body = serde_json::to_vec(&body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let url = format!("{}/v1internal:onboardUser", base_url.trim_end_matches('/'));
    for _ in 0..3 {
        let response = client
            .request(WreqMethod::POST, url.as_str())
            .bearer_auth(access_token)
            .header("user-agent", ANTIGRAVITY_USER_AGENT)
            .header("accept-encoding", "gzip")
            .header("content-type", "application/json")
            .body(body.clone())
            .send()
            .await
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
        let payload: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        if payload.get("done").and_then(|value| value.as_bool()) == Some(true) {
            let project_value = payload
                .get("response")
                .and_then(|value| value.get("cloudaicompanionProject"));
            let project_id = project_value
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned)
                .or_else(|| {
                    project_value
                        .and_then(|value| value.as_str())
                        .map(ToOwned::to_owned)
                });
            return Ok(project_id);
        }
    }
    Ok(None)
}

async fn get_onboard_tier(
    client: &WreqClient,
    access_token: &str,
    base_url: &str,
) -> Result<String, UpstreamError> {
    let payload = match call_code_assist(client, access_token, base_url).await {
        Ok(payload) => payload,
        Err(_) => return Ok("LEGACY".to_string()),
    };
    let tiers = payload
        .get("allowedTiers")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    for tier in tiers {
        let is_default = tier.get("isDefault").and_then(|value| value.as_bool());
        let id = tier.get("id").and_then(|value| value.as_str());
        if is_default == Some(true)
            && let Some(id) = id
        {
            return Ok(id.to_string());
        }
    }
    Ok("LEGACY".to_string())
}

async fn call_code_assist(
    client: &WreqClient,
    access_token: &str,
    base_url: &str,
) -> Result<serde_json::Value, UpstreamError> {
    let url = format!(
        "{}/v1internal:loadCodeAssist",
        base_url.trim_end_matches('/')
    );
    let body = json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });
    let body = serde_json::to_vec(&body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let response = client
        .request(WreqMethod::POST, url.as_str())
        .bearer_auth(access_token)
        .header("user-agent", ANTIGRAVITY_USER_AGENT)
        .header("accept-encoding", "gzip")
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !response.status().is_success() {
        return Err(UpstreamError::UpstreamRequest(format!(
            "loadCodeAssist failed: {}",
            response.status().as_u16()
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

async fn fetch_user_email(
    client: &WreqClient,
    access_token: &str,
    userinfo_url: &str,
) -> Result<Option<String>, UpstreamError> {
    let response = client
        .request(WreqMethod::GET, userinfo_url)
        .bearer_auth(access_token)
        .header("user-agent", ANTIGRAVITY_USER_AGENT)
        .send()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let payload: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    Ok(payload
        .get("email")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned))
}

fn build_authorize_url(
    auth_url: &str,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
) -> String {
    let query = {
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
        serializer.finish()
    };
    format!("{}?{}", auth_url.trim_end_matches('/'), query)
}

fn resolve_manual_code_and_state(
    query: Option<&str>,
) -> Result<(String, Option<String>), UpstreamError> {
    let direct_code = parse_query_value(query, "code");
    let direct_state = parse_query_value(query, "state");
    let callback_url = parse_query_value(query, "callback_url");

    if let Some(code) = direct_code {
        return Ok((code, direct_state));
    }

    if let Some(callback_url) = callback_url {
        let (code, state) = extract_code_state_from_callback_url(callback_url.as_str());
        if let Some(code) = code {
            return Ok((code, direct_state.or(state)));
        }
    }

    Err(UpstreamError::UpstreamRequest("missing code".to_string()))
}

fn extract_code_state_from_callback_url(value: &str) -> (Option<String>, Option<String>) {
    let Ok(parsed) = url::Url::parse(value) else {
        return (None, None);
    };
    let mut code = None;
    let mut state = None;
    for (name, value) in parsed.query_pairs() {
        if name == "code" {
            code = Some(value.into_owned());
            continue;
        }
        if name == "state" {
            state = Some(value.into_owned());
        }
    }
    (code, state)
}

fn generate_state_and_pkce() -> (String, String, String) {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    let state = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);

    rand::rng().fill(&mut bytes);
    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let digest = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (state, code_verifier, code_challenge)
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

fn json_oauth_response(status_code: u16, body: serde_json::Value) -> UpstreamOAuthResponse {
    json_oauth_response_with_meta(status_code, body, None)
}

fn json_oauth_response_with_meta(
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

fn json_oauth_error(status_code: u16, message: &str) -> UpstreamOAuthResponse {
    json_oauth_error_with_meta(status_code, message, None)
}

fn json_oauth_error_with_meta(
    status_code: u16,
    message: &str,
    request_meta: Option<UpstreamRequestMeta>,
) -> UpstreamOAuthResponse {
    json_oauth_response_with_meta(status_code, json!({ "error": message }), request_meta)
}

fn antigravity_oauth_authorize_url(settings: &ChannelSettings) -> &str {
    settings
        .oauth_authorize_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_AUTH_URL)
}

fn antigravity_oauth_token_url(settings: &ChannelSettings) -> &str {
    settings
        .oauth_token_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_TOKEN_URL)
}

fn antigravity_oauth_userinfo_url(settings: &ChannelSettings) -> &str {
    settings
        .oauth_userinfo_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(USERINFO_URL)
}
