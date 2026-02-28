use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use url::form_urlencoded;
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{
    CLIENT_ID, CLIENT_SECRET, DEFAULT_AUTH_URL, DEFAULT_AUTHORIZATION_CODE_REDIRECT_URI,
    DEFAULT_BASE_URL, DEFAULT_MANUAL_REDIRECT_URI, DEFAULT_TOKEN_URL, OAUTH_SCOPE,
    OAUTH_STATE_TTL_MS, TOKEN_REFRESH_SKEW_MS, USERINFO_URL, geminicli_user_agent,
};
use super::credential::GeminiCliCredential;
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
struct CachedGeminiCliToken {
    access_token: String,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct GeminiCliAuthMaterial {
    access_token: String,
    refresh_token: String,
    client_id: String,
    client_secret: String,
    pub(crate) project_id: String,
    expires_at_unix_ms: u64,
    user_email: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct GeminiCliRefreshedToken {
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    pub(crate) expires_at_unix_ms: u64,
    pub(crate) user_email: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct GeminiCliResolvedAccessToken {
    pub(crate) access_token: String,
    pub(crate) refreshed: Option<GeminiCliRefreshedToken>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum GeminiCliTokenRefreshError {
    #[error("invalid geminicli credential: {0}")]
    InvalidCredential(String),
    #[error("transient geminicli token refresh error: {0}")]
    Transient(String),
}

impl GeminiCliTokenRefreshError {
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

#[derive(Debug)]
enum ProjectResolutionFailure {
    ValidationRequired {
        reason: String,
        validation_url: String,
        learn_more_url: Option<String>,
    },
    IneligibleTiers {
        reasons: Vec<String>,
    },
    MissingProjectId,
    Upstream(String),
}

impl ProjectResolutionFailure {
    fn as_message(&self) -> String {
        match self {
            Self::ValidationRequired {
                reason,
                validation_url,
                learn_more_url,
            } => format!(
                "validation_required: reason={reason}, validation_url={validation_url}, learn_more_url={}",
                learn_more_url.as_deref().unwrap_or_default()
            ),
            Self::IneligibleTiers { reasons } => {
                format!("ineligible_tiers: {}", reasons.join("; "))
            }
            Self::MissingProjectId => "missing project_id (auto-detect failed)".to_string(),
            Self::Upstream(message) => message.clone(),
        }
    }

    fn into_oauth_response(self) -> UpstreamOAuthResponse {
        match self {
            Self::ValidationRequired {
                reason,
                validation_url,
                learn_more_url,
            } => json_oauth_response(
                403,
                json!({
                    "error": "validation_required",
                    "reason": reason,
                    "validation_url": validation_url,
                    "validation_learn_more_url": learn_more_url,
                }),
            ),
            Self::IneligibleTiers { reasons } => json_oauth_response(
                403,
                json!({
                    "error": "ineligible_tiers",
                    "reasons": reasons,
                }),
            ),
            Self::MissingProjectId => {
                json_oauth_error(400, "missing project_id (auto-detect failed)")
            }
            Self::Upstream(message) => json_oauth_response(
                502,
                json!({
                    "error": "codeassist_request_failed",
                    "message": message,
                }),
            ),
        }
    }
}

fn oauth_states() -> &'static DashMap<String, OAuthState> {
    static STATES: OnceLock<DashMap<String, OAuthState>> = OnceLock::new();
    STATES.get_or_init(DashMap::new)
}

fn geminicli_token_cache() -> &'static DashMap<String, CachedGeminiCliToken> {
    static CACHE: OnceLock<DashMap<String, CachedGeminiCliToken>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeminiCliOAuthMode {
    UserCode,
    AuthorizationCode,
}

fn parse_geminicli_oauth_mode(value: Option<&str>) -> Result<GeminiCliOAuthMode, &'static str> {
    let Some(value) = value else {
        return Ok(GeminiCliOAuthMode::UserCode);
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "authorization_code" => Ok(GeminiCliOAuthMode::AuthorizationCode),
        "user_code" => Ok(GeminiCliOAuthMode::UserCode),
        _ => Err("unsupported mode, expected user_code or authorization_code"),
    }
}

pub(crate) fn geminicli_auth_material_from_credential(
    credential: &GeminiCliCredential,
) -> Option<GeminiCliAuthMaterial> {
    let access_token = credential.access_token.trim().to_string();
    let refresh_token = credential.refresh_token.trim().to_string();
    let client_id = credential.client_id.trim().to_string();
    let client_secret = credential.client_secret.trim().to_string();

    if access_token.is_empty() && refresh_token.is_empty() {
        return None;
    }

    Some(GeminiCliAuthMaterial {
        access_token,
        refresh_token,
        client_id: if client_id.is_empty() {
            CLIENT_ID.to_string()
        } else {
            client_id
        },
        client_secret: if client_secret.is_empty() {
            CLIENT_SECRET.to_string()
        } else {
            client_secret
        },
        project_id: credential.project_id.clone(),
        expires_at_unix_ms: normalize_expires_at_ms(credential.expires_at),
        user_email: credential.user_email.clone(),
    })
}

pub async fn ensure_geminicli_project_id(
    client: &WreqClient,
    settings: &ChannelSettings,
    credential: &mut GeminiCliCredential,
) -> Result<(), UpstreamError> {
    if !credential.project_id.trim().is_empty() {
        return Ok(());
    }

    let now_unix_ms = current_unix_ms();
    let Some(material) = geminicli_auth_material_from_credential(credential) else {
        return Err(UpstreamError::SerializeRequest(
            "invalid geminicli credential: missing auth material".to_string(),
        ));
    };

    let resolved = resolve_geminicli_access_token(
        client,
        settings,
        "geminicli::project-detect",
        &material,
        now_unix_ms,
        false,
    )
    .await
    .map_err(|err| UpstreamError::UpstreamRequest(err.as_message()))?;

    if let Some(refreshed) = resolved.refreshed.as_ref() {
        credential.apply_token_refresh(
            refreshed.access_token.as_str(),
            refreshed.refresh_token.as_deref(),
            refreshed.expires_at_unix_ms,
            refreshed.user_email.as_deref(),
        );
    }

    let project_id = resolve_project_id(
        client,
        resolved.access_token.as_str(),
        geminicli_base_url(settings),
        None,
    )
    .await
    .map_err(|err| UpstreamError::UpstreamRequest(err.as_message()))?;

    let project_id = project_id.trim().to_string();
    if project_id.is_empty() {
        return Err(UpstreamError::SerializeRequest(
            "missing project_id (auto-detect failed)".to_string(),
        ));
    }
    credential.project_id = project_id;
    Ok(())
}

pub(crate) async fn resolve_geminicli_access_token(
    client: &WreqClient,
    settings: &ChannelSettings,
    cache_key: &str,
    material: &GeminiCliAuthMaterial,
    now_unix_ms: u64,
    force_refresh: bool,
) -> Result<GeminiCliResolvedAccessToken, GeminiCliTokenRefreshError> {
    if !force_refresh {
        if let Some(cached) = geminicli_token_cache().get(cache_key).filter(|item| {
            item.expires_at_unix_ms
                .saturating_sub(TOKEN_REFRESH_SKEW_MS)
                > now_unix_ms
        }) {
            return Ok(GeminiCliResolvedAccessToken {
                access_token: cached.access_token.clone(),
                refreshed: None,
            });
        }

        if access_token_valid(material, now_unix_ms) {
            geminicli_token_cache().insert(
                cache_key.to_string(),
                CachedGeminiCliToken {
                    access_token: material.access_token.clone(),
                    expires_at_unix_ms: material.expires_at_unix_ms,
                },
            );
            return Ok(GeminiCliResolvedAccessToken {
                access_token: material.access_token.clone(),
                refreshed: None,
            });
        }
    }

    let refreshed = refresh_access_token(client, settings, material, now_unix_ms).await?;
    geminicli_token_cache().insert(
        cache_key.to_string(),
        CachedGeminiCliToken {
            access_token: refreshed.access_token.clone(),
            expires_at_unix_ms: refreshed.expires_at_unix_ms,
        },
    );
    Ok(GeminiCliResolvedAccessToken {
        access_token: refreshed.access_token.clone(),
        refreshed: Some(refreshed),
    })
}

pub async fn execute_geminicli_oauth_start(
    _client: &WreqClient,
    settings: &ChannelSettings,
    request: &UpstreamOAuthRequest,
) -> Result<UpstreamOAuthResponse, UpstreamError> {
    let now_unix_ms = current_unix_ms();
    prune_oauth_states(now_unix_ms);

    let mode = match parse_geminicli_oauth_mode(
        parse_query_value(request.query.as_deref(), "mode").as_deref(),
    ) {
        Ok(mode) => mode,
        Err(message) => return Ok(json_oauth_error(400, message)),
    };
    let redirect_uri =
        parse_query_value(request.query.as_deref(), "redirect_uri").unwrap_or_else(|| match mode {
            GeminiCliOAuthMode::UserCode => DEFAULT_MANUAL_REDIRECT_URI.to_string(),
            GeminiCliOAuthMode::AuthorizationCode => {
                DEFAULT_AUTHORIZATION_CODE_REDIRECT_URI.to_string()
            }
        });
    let project_id = parse_query_value(request.query.as_deref(), "project_id");
    let (state, code_verifier, code_challenge) = generate_state_and_pkce();
    let auth_url = build_authorize_url(
        geminicli_oauth_authorize_url(settings),
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

    let (mode_name, instructions) = match mode {
        GeminiCliOAuthMode::UserCode => (
            "user_code",
            "Open auth_url, copy the authorization code, then call /oauth/callback with code/state (or callback_url).",
        ),
        GeminiCliOAuthMode::AuthorizationCode => (
            "authorization_code",
            "Open auth_url and complete browser authorization, then call /oauth/callback with code/state (or callback_url).",
        ),
    };
    Ok(json_oauth_response(
        200,
        json!({
            "auth_url": auth_url,
            "state": state,
            "redirect_uri": redirect_uri,
            "mode": mode_name,
            "instructions": instructions,
        }),
    ))
}

pub async fn execute_geminicli_oauth_callback(
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

    let callback_mode = match parse_query_value(request.query.as_deref(), "mode") {
        Some(mode) => match parse_geminicli_oauth_mode(Some(mode.as_str())) {
            Ok(value) => Some(value),
            Err(message) => {
                return Ok(UpstreamOAuthCallbackResult {
                    response: json_oauth_error(400, message),
                    credential: None,
                });
            }
        },
        None => None,
    };

    let (code, state_param) =
        match resolve_manual_code_and_state(request.query.as_deref(), callback_mode) {
            Ok(value) => value,
            Err(message) => {
                return Ok(UpstreamOAuthCallbackResult {
                    response: json_oauth_error(400, message),
                    credential: None,
                });
            }
        };

    let oauth_state = if let Some(state) = state_param.as_deref() {
        oauth_states().remove(state).map(|(_, value)| value)
    } else {
        if oauth_states().is_empty() {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "missing state"),
                credential: None,
            });
        }
        if oauth_states().len() > 1 {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "ambiguous_state"),
                credential: None,
            });
        }
        let Some(entry) = oauth_states().iter().next() else {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, "missing state"),
                credential: None,
            });
        };
        let key = entry.key().clone();
        oauth_states().remove(key.as_str()).map(|(_, value)| value)
    };

    let Some(oauth_state) = oauth_state else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, "missing state"),
            credential: None,
        });
    };

    let (token, token_request_meta) = match exchange_code_for_tokens(
        client,
        geminicli_oauth_token_url(settings),
        code.as_str(),
        oauth_state.redirect_uri.as_str(),
        oauth_state.code_verifier.as_str(),
    )
    .await
    {
        Ok(token) => token,
        Err(err) => {
            return Ok(UpstreamOAuthCallbackResult {
                response: json_oauth_error(400, err.to_string().as_str()),
                credential: None,
            });
        }
    };

    let Some(access_token) = token
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
    else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error_with_meta(
                400,
                "missing access_token",
                Some(token_request_meta.clone()),
            ),
            credential: None,
        });
    };
    let Some(refresh_token) = token
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
    else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error_with_meta(
                400,
                "missing refresh_token",
                Some(token_request_meta.clone()),
            ),
            credential: None,
        });
    };

    let project_hint = oauth_state
        .project_id
        .or_else(|| parse_query_value(request.query.as_deref(), "project_id"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let project_id = match resolve_project_id(
        client,
        access_token.as_str(),
        geminicli_base_url(settings),
        project_hint.as_deref(),
    )
    .await
    {
        Ok(project_id) => project_id,
        Err(failure) => {
            let mut response = failure.into_oauth_response();
            response.request_meta = Some(token_request_meta.clone());
            return Ok(UpstreamOAuthCallbackResult {
                response,
                credential: None,
            });
        }
    };

    let user_email = fetch_user_email(
        client,
        access_token.as_str(),
        geminicli_oauth_userinfo_url(settings),
    )
    .await
    .ok()
    .flatten();
    let expires_at_unix_ms =
        now_unix_ms.saturating_add(token.expires_in.unwrap_or(3600).saturating_mul(1000));

    let credential = UpstreamOAuthCredential {
        label: parse_query_value(request.query.as_deref(), "label"),
        credential: ChannelCredential::Builtin(BuiltinChannelCredential::GeminiCli(
            GeminiCliCredential {
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
            }),
            Some(token_request_meta),
        ),
        credential: Some(credential),
    })
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn geminicli_base_url(settings: &ChannelSettings) -> &str {
    if settings.base_url().trim().is_empty() {
        DEFAULT_BASE_URL
    } else {
        settings.base_url().trim()
    }
}

fn normalize_expires_at_ms(value: i64) -> u64 {
    value.max(0) as u64
}

fn access_token_valid(material: &GeminiCliAuthMaterial, now_unix_ms: u64) -> bool {
    !material.access_token.trim().is_empty()
        && material
            .expires_at_unix_ms
            .saturating_sub(TOKEN_REFRESH_SKEW_MS)
            > now_unix_ms
}

fn prune_oauth_states(now_unix_ms: u64) {
    oauth_states().retain(|_, value| {
        now_unix_ms.saturating_sub(value.created_at_unix_ms) <= OAUTH_STATE_TTL_MS
    });
}

fn generate_state_and_pkce() -> (String, String, String) {
    let mut bytes = rand::random::<[u8; 32]>();
    let state = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);

    bytes = rand::random::<[u8; 32]>();
    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let digest = Sha256::digest(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

    (state, code_verifier, code_challenge)
}

fn build_authorize_url(
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

fn resolve_manual_code_and_state(
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

fn extract_code_state_from_callback_url(value: &str) -> (Option<String>, Option<String>) {
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

fn extract_code_state_from_query(query: &str) -> (Option<String>, Option<String>) {
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

fn percent_decode_component(value: &str) -> String {
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

async fn refresh_access_token(
    client: &WreqClient,
    settings: &ChannelSettings,
    material: &GeminiCliAuthMaterial,
    now_unix_ms: u64,
) -> Result<GeminiCliRefreshedToken, GeminiCliTokenRefreshError> {
    let refresh_token = material.refresh_token.trim();
    if refresh_token.is_empty() {
        return Err(GeminiCliTokenRefreshError::InvalidCredential(
            "missing refresh_token".to_string(),
        ));
    }
    if material.client_id.trim().is_empty() || material.client_secret.trim().is_empty() {
        return Err(GeminiCliTokenRefreshError::InvalidCredential(
            "missing client credentials".to_string(),
        ));
    }

    let body = {
        let mut serializer = form_urlencoded::Serializer::new(String::new());
        serializer
            .append_pair("refresh_token", refresh_token)
            .append_pair("client_id", material.client_id.as_str())
            .append_pair("client_secret", material.client_secret.as_str())
            .append_pair("grant_type", "refresh_token");
        serializer.finish()
    };
    let (response, _request_meta) =
        send_token_request(client, geminicli_oauth_token_url(settings), body.as_str())
            .await
            .map_err(|err| GeminiCliTokenRefreshError::Transient(err.to_string()))?;

    let access_token = response
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            GeminiCliTokenRefreshError::Transient("token response missing access_token".to_string())
        })?;

    let expires_at_unix_ms = now_unix_ms
        .saturating_add(response.expires_in.unwrap_or(3600).saturating_mul(1000))
        .max(now_unix_ms + 60_000);
    let refresh_token = response
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let user_email = if material
        .user_email
        .as_deref()
        .map(str::trim)
        .is_none_or(|value| value.is_empty())
    {
        fetch_user_email(
            client,
            access_token.as_str(),
            geminicli_oauth_userinfo_url(settings),
        )
        .await
        .ok()
        .flatten()
    } else {
        None
    };

    Ok(GeminiCliRefreshedToken {
        access_token,
        refresh_token,
        expires_at_unix_ms,
        user_email,
    })
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
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;

    let token = serde_json::from_slice::<TokenResponse>(&bytes)
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !status.is_success() {
        let message = token_response_error(status.as_u16(), &token, &bytes);
        return Err(UpstreamError::UpstreamRequest(message));
    }
    Ok((token, request_meta))
}

fn token_response_error(status_code: u16, token: &TokenResponse, bytes: &[u8]) -> String {
    if let Some(error) = token.error.as_deref() {
        let detail = token.error_description.as_deref().unwrap_or_default();
        return format!("oauth token failed: {status_code} {error} {detail}");
    }
    format!(
        "oauth token failed: {status_code} {}",
        String::from_utf8_lossy(bytes)
    )
}

fn validation_required_from_payload(
    payload: &serde_json::Value,
) -> Option<ProjectResolutionFailure> {
    let tiers = payload
        .get("ineligibleTiers")
        .and_then(serde_json::Value::as_array)?;
    for tier in tiers {
        let reason_code = tier
            .get("reasonCode")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if reason_code != "VALIDATION_REQUIRED" {
            continue;
        }
        let validation_url = tier
            .get("validationUrl")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?
            .to_string();
        let reason = tier
            .get("reasonMessage")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("account validation required")
            .to_string();
        let learn_more_url = tier
            .get("validationLearnMoreUrl")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        return Some(ProjectResolutionFailure::ValidationRequired {
            reason,
            validation_url,
            learn_more_url,
        });
    }
    None
}

fn ineligible_reasons_from_payload(payload: &serde_json::Value) -> Vec<String> {
    payload
        .get("ineligibleTiers")
        .and_then(serde_json::Value::as_array)
        .map(|tiers| {
            tiers
                .iter()
                .filter_map(|tier| {
                    tier.get("reasonMessage")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn has_current_tier(payload: &serde_json::Value) -> bool {
    payload
        .get("currentTier")
        .map(|value| !value.is_null())
        .unwrap_or(false)
}

fn default_onboard_tier_id(payload: &serde_json::Value) -> String {
    if let Some(tiers) = payload
        .get("allowedTiers")
        .and_then(serde_json::Value::as_array)
    {
        for tier in tiers {
            if tier.get("isDefault").and_then(serde_json::Value::as_bool) == Some(true)
                && let Some(id) = tier
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            {
                return id.to_string();
            }
        }
    }
    "legacy-tier".to_string()
}

async fn resolve_project_id(
    client: &WreqClient,
    access_token: &str,
    base_url: &str,
    project_id: Option<&str>,
) -> Result<String, ProjectResolutionFailure> {
    let load_payload = load_code_assist_payload(client, access_token, base_url, project_id)
        .await
        .map_err(|err| ProjectResolutionFailure::Upstream(err.to_string()))?;

    if let Some(payload) = load_payload.as_ref() {
        if let Some(validation_required) = validation_required_from_payload(payload) {
            return Err(validation_required);
        }
        let ineligible_reasons = ineligible_reasons_from_payload(payload);

        if has_current_tier(payload) {
            if let Some(project) = payload
                .get("cloudaicompanionProject")
                .and_then(parse_project_id_value)
            {
                return Ok(project);
            }
            if let Some(project) = project_id {
                return Ok(project.to_string());
            }
            if !ineligible_reasons.is_empty() {
                return Err(ProjectResolutionFailure::IneligibleTiers {
                    reasons: ineligible_reasons,
                });
            }
            return Err(ProjectResolutionFailure::MissingProjectId);
        }

        let tier_id = default_onboard_tier_id(payload);
        let onboarded =
            onboard_user_project(client, access_token, base_url, tier_id.as_str(), project_id)
                .await
                .map_err(|err| ProjectResolutionFailure::Upstream(err.to_string()))?;
        if let Some(project) = onboarded {
            return Ok(project);
        }
        if let Some(project) = project_id {
            return Ok(project.to_string());
        }
        if !ineligible_reasons.is_empty() {
            return Err(ProjectResolutionFailure::IneligibleTiers {
                reasons: ineligible_reasons,
            });
        }
        return Err(ProjectResolutionFailure::MissingProjectId);
    }

    let onboarded = onboard_user_project(client, access_token, base_url, "legacy-tier", project_id)
        .await
        .map_err(|err| ProjectResolutionFailure::Upstream(err.to_string()))?;
    if let Some(project) = onboarded {
        return Ok(project);
    }
    if let Some(project) = project_id {
        return Ok(project.to_string());
    }
    Err(ProjectResolutionFailure::MissingProjectId)
}

fn code_assist_metadata(project_id: Option<&str>) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("ideType".to_string(), json!("IDE_UNSPECIFIED"));
    metadata.insert("platform".to_string(), json!("PLATFORM_UNSPECIFIED"));
    metadata.insert("pluginType".to_string(), json!("GEMINI"));
    if let Some(project) = project_id.map(str::trim).filter(|value| !value.is_empty()) {
        metadata.insert("duetProject".to_string(), json!(project));
    }
    serde_json::Value::Object(metadata)
}

fn parse_project_id_value(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

async fn load_code_assist_payload(
    client: &WreqClient,
    access_token: &str,
    base_url: &str,
    project_id: Option<&str>,
) -> Result<Option<serde_json::Value>, UpstreamError> {
    let url = format!(
        "{}/v1internal:loadCodeAssist",
        base_url.trim_end_matches('/')
    );
    let body = json!({
        "cloudaicompanionProject": project_id,
        "metadata": code_assist_metadata(project_id),
    });
    let user_agent = geminicli_user_agent(None);
    let response = client
        .request(WreqMethod::POST, url.as_str())
        .bearer_auth(access_token)
        .header("user-agent", user_agent.as_str())
        .header("accept-encoding", "gzip")
        .header("content-type", "application/json")
        .body(
            serde_json::to_vec(&body)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
        )
        .send()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let body = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let payload: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    Ok(Some(payload))
}

async fn onboard_user_project(
    client: &WreqClient,
    access_token: &str,
    base_url: &str,
    tier_id: &str,
    project_id: Option<&str>,
) -> Result<Option<String>, UpstreamError> {
    let url = format!("{}/v1internal:onboardUser", base_url.trim_end_matches('/'));
    let project_for_request = if tier_id.eq_ignore_ascii_case("free-tier") {
        None
    } else {
        project_id
    };
    let body = json!({
        "tierId": tier_id,
        "cloudaicompanionProject": project_for_request,
        "metadata": code_assist_metadata(project_for_request),
    });
    let body = serde_json::to_vec(&body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let user_agent = geminicli_user_agent(None);
    let response = client
        .request(WreqMethod::POST, url.as_str())
        .bearer_auth(access_token)
        .header("user-agent", user_agent.as_str())
        .header("accept-encoding", "gzip")
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let response_bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let mut payload: serde_json::Value = serde_json::from_slice(&response_bytes)
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    for _ in 0..5 {
        if payload.get("done").and_then(serde_json::Value::as_bool) == Some(true) {
            let project = payload
                .get("response")
                .and_then(|value| value.get("cloudaicompanionProject"))
                .and_then(parse_project_id_value);
            return Ok(project);
        }
        let Some(name) = payload
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            break;
        };
        let operation_url = format!("{}/v1internal/{name}", base_url.trim_end_matches('/'));
        let poll_response = client
            .request(WreqMethod::GET, operation_url.as_str())
            .bearer_auth(access_token)
            .header("user-agent", user_agent.as_str())
            .header("accept-encoding", "gzip")
            .send()
            .await
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
        if !poll_response.status().is_success() {
            break;
        }
        let poll_bytes = poll_response
            .bytes()
            .await
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
        payload = serde_json::from_slice(&poll_bytes)
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    }
    Ok(None)
}

fn parse_user_email(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("email")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn fetch_user_email(
    client: &WreqClient,
    access_token: &str,
    userinfo_url: &str,
) -> Result<Option<String>, UpstreamError> {
    let user_agent = geminicli_user_agent(None);
    let response = client
        .request(WreqMethod::GET, userinfo_url)
        .bearer_auth(access_token)
        .header("user-agent", user_agent.as_str())
        .send()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let body = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let payload = serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    Ok(parse_user_email(&payload))
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

fn geminicli_oauth_authorize_url(settings: &ChannelSettings) -> &str {
    settings
        .oauth_authorize_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_AUTH_URL)
}

fn geminicli_oauth_token_url(settings: &ChannelSettings) -> &str {
    settings
        .oauth_token_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_TOKEN_URL)
}

fn geminicli_oauth_userinfo_url(settings: &ChannelSettings) -> &str {
    settings
        .oauth_userinfo_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(USERINFO_URL)
}
