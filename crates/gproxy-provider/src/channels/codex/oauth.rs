use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use dashmap::DashMap;
use rand::Rng;
use serde::Deserialize;
use serde::de::{self, Deserializer};
use serde_json::{Value, json};
use sha2::Digest;
use url::form_urlencoded;
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{
    CLIENT_ID, DEFAULT_BROWSER_REDIRECT_URI, DEFAULT_ISSUER, OAUTH_ORIGINATOR, OAUTH_SCOPE,
    OAUTH_STATE_TTL_MS, TOKEN_REFRESH_SKEW_MS,
};
use super::credential::CodexCredential;
use crate::channels::ChannelSettings;
use crate::channels::upstream::{
    UpstreamError, UpstreamOAuthCallbackResult, UpstreamOAuthCredential, UpstreamOAuthRequest,
    UpstreamOAuthResponse, UpstreamRequestMeta, tracked_request, tracked_send_request,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};

#[derive(Debug, Clone)]
enum OAuthMode {
    DeviceAuth,
    AuthorizationCode,
}

#[derive(Debug, Clone)]
enum OAuthState {
    DeviceAuth {
        device_auth_id: String,
        user_code: String,
        interval_secs: u64,
        created_at_unix_ms: u64,
    },
    AuthorizationCode {
        code_verifier: String,
        redirect_uri: String,
        created_at_unix_ms: u64,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct CodexAuthMaterial {
    access_token: String,
    refresh_token: String,
    id_token: String,
    pub(crate) account_id: String,
    expires_at_unix_ms: u64,
}

impl CodexAuthMaterial {
    fn access_token_valid(&self, now_unix_ms: u64) -> bool {
        !self.access_token.trim().is_empty()
            && self
                .expires_at_unix_ms
                .saturating_sub(TOKEN_REFRESH_SKEW_MS)
                > now_unix_ms
    }

    fn can_refresh(&self) -> bool {
        !self.refresh_token.trim().is_empty()
    }
}

#[derive(Debug, Clone)]
struct CachedCodexToken {
    access_token: String,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexRefreshedToken {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) expires_at_unix_ms: u64,
    pub(crate) user_email: Option<String>,
    pub(crate) id_token: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexResolvedAccessToken {
    pub(crate) access_token: String,
    pub(crate) refreshed: Option<CodexRefreshedToken>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CodexTokenRefreshError {
    #[error("invalid codex credential: {0}")]
    InvalidCredential(String),
    #[error("transient codex token refresh error: {0}")]
    Transient(String),
}

impl CodexTokenRefreshError {
    pub(crate) fn as_message(&self) -> String {
        self.to_string()
    }

    pub(crate) fn is_invalid_credential(&self) -> bool {
        matches!(self, Self::InvalidCredential(_))
    }
}

#[derive(Debug, Deserialize)]
struct DeviceUserCodeResponse {
    device_auth_id: String,
    #[serde(alias = "user_code", alias = "usercode")]
    user_code: String,
    #[serde(
        default = "default_poll_interval_secs",
        deserialize_with = "deserialize_u64_from_string"
    )]
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct DeviceTokenPollResponse {
    authorization_code: String,
    code_verifier: String,
}

#[derive(Debug)]
enum DeviceAuthPollStatus {
    Pending,
    Authorized(DeviceTokenPollResponse),
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_u64_from_string")]
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum U64StringOrNumber {
    String(String),
    Number(u64),
}

fn deserialize_u64_from_string<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = U64StringOrNumber::deserialize(deserializer)?;
    match value {
        U64StringOrNumber::String(value) => value
            .trim()
            .parse::<u64>()
            .map_err(|err| de::Error::custom(format!("invalid u64 string: {err}"))),
        U64StringOrNumber::Number(value) => Ok(value),
    }
}

fn deserialize_option_u64_from_string<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<U64StringOrNumber>::deserialize(deserializer)?;
    value
        .map(|item| match item {
            U64StringOrNumber::String(value) => value
                .trim()
                .parse::<u64>()
                .map_err(|err| de::Error::custom(format!("invalid u64 string: {err}"))),
            U64StringOrNumber::Number(value) => Ok(value),
        })
        .transpose()
}

#[derive(Debug, Default)]
struct IdTokenClaims {
    email: Option<String>,
    plan: Option<String>,
    account_id: Option<String>,
}

fn oauth_states() -> &'static DashMap<String, OAuthState> {
    static STATES: OnceLock<DashMap<String, OAuthState>> = OnceLock::new();
    STATES.get_or_init(DashMap::new)
}

fn codex_token_cache() -> &'static DashMap<String, CachedCodexToken> {
    static CACHE: OnceLock<DashMap<String, CachedCodexToken>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

pub(crate) fn codex_auth_material_from_credential(
    value: &CodexCredential,
) -> Option<CodexAuthMaterial> {
    let account_id = value.account_id.trim();
    if account_id.is_empty() {
        return None;
    }

    Some(CodexAuthMaterial {
        access_token: value.access_token.trim().to_string(),
        refresh_token: value.refresh_token.trim().to_string(),
        id_token: value.id_token.trim().to_string(),
        account_id: account_id.to_string(),
        expires_at_unix_ms: normalize_expires_at_ms(value.expires_at),
    })
}

pub(crate) async fn resolve_codex_access_token(
    client: &WreqClient,
    settings: &ChannelSettings,
    cache_key: &str,
    material: &CodexAuthMaterial,
    now_unix_ms: u64,
    force_refresh: bool,
) -> Result<CodexResolvedAccessToken, CodexTokenRefreshError> {
    if !force_refresh {
        if let Some(cached) = codex_token_cache().get(cache_key).filter(|item| {
            item.expires_at_unix_ms
                .saturating_sub(TOKEN_REFRESH_SKEW_MS)
                > now_unix_ms
        }) {
            return Ok(CodexResolvedAccessToken {
                access_token: cached.access_token.clone(),
                refreshed: None,
            });
        }

        if material.access_token_valid(now_unix_ms) {
            codex_token_cache().insert(
                cache_key.to_string(),
                CachedCodexToken {
                    access_token: material.access_token.clone(),
                    expires_at_unix_ms: material.expires_at_unix_ms,
                },
            );
            return Ok(CodexResolvedAccessToken {
                access_token: material.access_token.clone(),
                refreshed: None,
            });
        }
    }

    let issuer = codex_oauth_issuer_from_settings(settings);
    let refreshed = refresh_access_token(client, issuer.as_str(), material, now_unix_ms).await?;
    codex_token_cache().insert(
        cache_key.to_string(),
        CachedCodexToken {
            access_token: refreshed.access_token.clone(),
            expires_at_unix_ms: refreshed.expires_at_unix_ms,
        },
    );
    Ok(CodexResolvedAccessToken {
        access_token: refreshed.access_token.clone(),
        refreshed: Some(refreshed),
    })
}

pub async fn execute_codex_oauth_start(
    client: &WreqClient,
    settings: &ChannelSettings,
    request: &UpstreamOAuthRequest,
) -> Result<UpstreamOAuthResponse, UpstreamError> {
    let now_unix_ms = current_unix_ms();
    prune_oauth_states(now_unix_ms);
    let issuer = codex_oauth_issuer(settings, request.query.as_deref());

    let mode = parse_oauth_mode(parse_query_value(request.query.as_deref(), "mode").as_deref());
    let state_id = generate_oauth_state();

    match mode {
        OAuthMode::DeviceAuth => {
            let (user_code, request_meta) =
                request_device_user_code(client, issuer.as_str()).await?;
            oauth_states().insert(
                state_id.clone(),
                OAuthState::DeviceAuth {
                    device_auth_id: user_code.device_auth_id.clone(),
                    user_code: user_code.user_code.clone(),
                    interval_secs: user_code.interval.max(1),
                    created_at_unix_ms: now_unix_ms,
                },
            );

            let verification_uri = format!("{}/codex/device", issuer.trim_end_matches('/'));
            Ok(json_oauth_response_with_meta(
                200,
                json!({
                    "auth_url": verification_uri,
                    "verification_uri": format!("{}/codex/device", issuer.trim_end_matches('/')),
                    "user_code": user_code.user_code,
                    "interval": user_code.interval.max(1),
                    "state": state_id,
                    "mode": "device_auth",
                    "instructions": "Open verification_uri, enter user_code, then call /oauth/callback with state.",
                }),
                Some(request_meta),
            ))
        }
        OAuthMode::AuthorizationCode => {
            let code_verifier = generate_code_verifier();
            let code_challenge = generate_code_challenge(code_verifier.as_str());
            let redirect_uri = parse_query_value(request.query.as_deref(), "redirect_uri")
                .unwrap_or_else(|| DEFAULT_BROWSER_REDIRECT_URI.to_string());
            let scope = parse_query_value(request.query.as_deref(), "scope")
                .unwrap_or_else(|| OAUTH_SCOPE.to_string());
            let originator = parse_query_value(request.query.as_deref(), "originator")
                .unwrap_or_else(|| OAUTH_ORIGINATOR.to_string());
            let allowed_workspace_id =
                parse_query_value(request.query.as_deref(), "allowed_workspace_id");
            let auth_url = build_authorize_url(
                issuer.as_str(),
                redirect_uri.as_str(),
                scope.as_str(),
                originator.as_str(),
                code_challenge.as_str(),
                state_id.as_str(),
                allowed_workspace_id.as_deref(),
            );

            oauth_states().insert(
                state_id.clone(),
                OAuthState::AuthorizationCode {
                    code_verifier,
                    redirect_uri: redirect_uri.clone(),
                    created_at_unix_ms: now_unix_ms,
                },
            );

            Ok(json_oauth_response(
                200,
                json!({
                    "auth_url": auth_url,
                    "state": state_id,
                    "redirect_uri": redirect_uri,
                    "scope": scope,
                    "mode": "authorization_code",
                    "instructions": "Open auth_url, then call /oauth/callback with code/state (or callback_url).",
                }),
            ))
        }
    }
}

pub async fn execute_codex_oauth_callback(
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
    let issuer = codex_oauth_issuer(settings, request.query.as_deref());

    let state_param = parse_query_value(request.query.as_deref(), "state").or_else(|| {
        parse_query_value(request.query.as_deref(), "callback_url")
            .and_then(|url| extract_code_state_from_callback_url(url.as_str()).1)
    });

    let (state_id, oauth_state) = if let Some(state) = state_param {
        (
            state.clone(),
            oauth_states()
                .get(state.as_str())
                .map(|entry| entry.clone()),
        )
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
        (entry.key().clone(), Some(entry.value().clone()))
    };

    let Some(oauth_state) = oauth_state else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error(400, "missing state"),
            credential: None,
        });
    };

    match oauth_state {
        OAuthState::DeviceAuth {
            device_auth_id,
            user_code,
            interval_secs,
            ..
        } => {
            let poll_status = poll_device_authorization(
                client,
                issuer.as_str(),
                device_auth_id.as_str(),
                user_code.as_str(),
            )
            .await?;
            let (poll_status, poll_request_meta) = poll_status;
            let poll_success = match poll_status {
                DeviceAuthPollStatus::Pending => {
                    let message = format!(
                        "authorization_pending: retry after {}s",
                        interval_secs.max(1)
                    );
                    return Ok(UpstreamOAuthCallbackResult {
                        response: json_oauth_error_with_meta(
                            409,
                            message.as_str(),
                            Some(poll_request_meta),
                        ),
                        credential: None,
                    });
                }
                DeviceAuthPollStatus::Authorized(data) => data,
            };

            oauth_states().remove(state_id.as_str());
            let redirect_uri = format!("{}/deviceauth/callback", issuer.trim_end_matches('/'));
            let (tokens, request_meta) = exchange_code_for_tokens(
                client,
                issuer.as_str(),
                redirect_uri.as_str(),
                poll_success.code_verifier.as_str(),
                poll_success.authorization_code.as_str(),
            )
            .await?;
            build_callback_result(tokens, Some(request_meta))
        }
        OAuthState::AuthorizationCode {
            code_verifier,
            redirect_uri,
            ..
        } => {
            let (code, callback_state) =
                match resolve_manual_code_and_state(request.query.as_deref()) {
                    Ok(value) => value,
                    Err(message) => {
                        return Ok(UpstreamOAuthCallbackResult {
                            response: json_oauth_error(400, message),
                            credential: None,
                        });
                    }
                };

            if let Some(callback_state) = callback_state
                && callback_state != state_id
            {
                return Ok(UpstreamOAuthCallbackResult {
                    response: json_oauth_error(400, "state_mismatch"),
                    credential: None,
                });
            }

            oauth_states().remove(state_id.as_str());
            let (tokens, request_meta) = exchange_code_for_tokens(
                client,
                issuer.as_str(),
                redirect_uri.as_str(),
                code_verifier.as_str(),
                code.as_str(),
            )
            .await?;
            build_callback_result(tokens, Some(request_meta))
        }
    }
}

fn parse_oauth_mode(value: Option<&str>) -> OAuthMode {
    let Some(raw) = value else {
        return OAuthMode::DeviceAuth;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "authorization_code" | "auth_code" | "pkce" | "browser" | "browser_auth" => {
            OAuthMode::AuthorizationCode
        }
        _ => OAuthMode::DeviceAuth,
    }
}

fn generate_oauth_state() -> String {
    let mut state_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut state_bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(state_bytes)
}

fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 64];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn generate_code_challenge(code_verifier: &str) -> String {
    let digest = sha2::Sha256::digest(code_verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn build_authorize_url(
    issuer: &str,
    redirect_uri: &str,
    scope: &str,
    originator: &str,
    code_challenge: &str,
    state: &str,
    allowed_workspace_id: Option<&str>,
) -> String {
    let mut query = vec![
        ("response_type".to_string(), "code".to_string()),
        ("client_id".to_string(), CLIENT_ID.to_string()),
        ("redirect_uri".to_string(), redirect_uri.to_string()),
        ("scope".to_string(), scope.to_string()),
        ("code_challenge".to_string(), code_challenge.to_string()),
        ("code_challenge_method".to_string(), "S256".to_string()),
        ("id_token_add_organizations".to_string(), "true".to_string()),
        ("codex_cli_simplified_flow".to_string(), "true".to_string()),
        ("state".to_string(), state.to_string()),
        ("originator".to_string(), originator.to_string()),
    ];
    if let Some(workspace_id) = allowed_workspace_id
        && !workspace_id.trim().is_empty()
    {
        query.push(("allowed_workspace_id".to_string(), workspace_id.to_string()));
    }
    let qs = query
        .into_iter()
        .map(|(key, value)| {
            format!(
                "{key}={}",
                form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{}/oauth/authorize?{qs}", issuer.trim_end_matches('/'))
}

fn resolve_manual_code_and_state(
    query: Option<&str>,
) -> Result<(String, Option<String>), &'static str> {
    let mut code = parse_query_value(query, "code");
    let mut state = parse_query_value(query, "state");
    if let Some(callback_url) = parse_query_value(query, "callback_url") {
        let (code_from_callback, state_from_callback) =
            extract_code_state_from_callback_url(callback_url.as_str());
        if code.is_none() {
            code = code_from_callback;
        }
        if state.is_none() {
            state = state_from_callback;
        }
    }

    let Some(code) = code.filter(|value| !value.trim().is_empty()) else {
        return Err("missing code");
    };
    Ok((code, state))
}

fn extract_code_state_from_callback_url(callback_url: &str) -> (Option<String>, Option<String>) {
    let raw = callback_url.trim();
    if raw.is_empty() {
        return (None, None);
    }

    let query = if let Some((_, query)) = raw.split_once('?') {
        query
    } else {
        raw
    };
    (
        parse_query_value(Some(query), "code"),
        parse_query_value(Some(query), "state"),
    )
}

fn parse_query_value(query: Option<&str>, key: &str) -> Option<String> {
    let raw = query?.trim().trim_start_matches('?');
    for (name, value) in form_urlencoded::parse(raw.as_bytes()) {
        if name == key {
            return Some(value.into_owned());
        }
    }
    None
}

fn codex_oauth_issuer(settings: &ChannelSettings, query: Option<&str>) -> String {
    parse_query_value(query, "oauth_issuer")
        .or_else(|| parse_query_value(query, "issuer"))
        .or_else(|| {
            settings
                .oauth_issuer_url()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| DEFAULT_ISSUER.to_string())
}

fn codex_oauth_issuer_from_settings(settings: &ChannelSettings) -> String {
    settings
        .oauth_issuer_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_ISSUER)
        .to_string()
}

fn prune_oauth_states(now_unix_ms: u64) {
    let states = oauth_states();
    let expired = states
        .iter()
        .filter_map(|entry| {
            let created_at_unix_ms = match entry.value() {
                OAuthState::DeviceAuth {
                    created_at_unix_ms, ..
                }
                | OAuthState::AuthorizationCode {
                    created_at_unix_ms, ..
                } => *created_at_unix_ms,
            };
            if now_unix_ms.saturating_sub(created_at_unix_ms) > OAUTH_STATE_TTL_MS {
                Some(entry.key().clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    for key in expired {
        states.remove(key.as_str());
    }
}

fn json_oauth_response(status_code: u16, body: Value) -> UpstreamOAuthResponse {
    json_oauth_response_with_meta(status_code, body, None)
}

fn json_oauth_response_with_meta(
    status_code: u16,
    body: Value,
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

fn build_callback_result(
    tokens: TokenResponse,
    request_meta: Option<UpstreamRequestMeta>,
) -> Result<UpstreamOAuthCallbackResult, UpstreamError> {
    let Some(access_token) = tokens
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
    else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error_with_meta(400, "missing_access_token", request_meta.clone()),
            credential: None,
        });
    };

    let Some(refresh_token) = tokens
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
    else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error_with_meta(
                400,
                "missing_refresh_token",
                request_meta.clone(),
            ),
            credential: None,
        });
    };

    let Some(id_token) = tokens
        .id_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
    else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error_with_meta(400, "missing_id_token", request_meta.clone()),
            credential: None,
        });
    };

    let claims = parse_id_token_claims(id_token.as_str());
    let Some(account_id) = claims.account_id.clone() else {
        return Ok(UpstreamOAuthCallbackResult {
            response: json_oauth_error_with_meta(400, "missing_account_id", request_meta.clone()),
            credential: None,
        });
    };

    let expires_at_unix_ms =
        current_unix_ms().saturating_add(tokens.expires_in.unwrap_or(3600).saturating_mul(1000));

    let credential = UpstreamOAuthCredential {
        label: claims
            .email
            .clone()
            .or_else(|| Some(format!("codex:{account_id}"))),
        credential: ChannelCredential::Builtin(BuiltinChannelCredential::Codex(CodexCredential {
            access_token: access_token.clone(),
            refresh_token: refresh_token.clone(),
            id_token: id_token.clone(),
            user_email: claims.email.clone(),
            account_id: account_id.clone(),
            expires_at: expires_at_unix_ms.min(i64::MAX as u64) as i64,
        })),
    };

    Ok(UpstreamOAuthCallbackResult {
        response: json_oauth_response_with_meta(
            200,
            json!({
                "access_token": access_token,
                "refresh_token": refresh_token,
                "id_token": id_token,
                "account_id": account_id,
                "email": claims.email,
                "plan": claims.plan,
                "expires_at_unix_ms": expires_at_unix_ms,
            }),
            request_meta,
        ),
        credential: Some(credential),
    })
}

async fn request_device_user_code(
    client: &WreqClient,
    issuer: &str,
) -> Result<(DeviceUserCodeResponse, UpstreamRequestMeta), UpstreamError> {
    let body = serde_json::to_vec(&json!({ "client_id": CLIENT_ID }))
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let url = format!(
        "{}/api/accounts/deviceauth/usercode",
        issuer.trim_end_matches('/')
    );
    let headers = vec![("content-type".to_string(), "application/json".to_string())];
    let (response, request_meta) =
        tracked_send_request(client, WreqMethod::POST, url.as_str(), headers, Some(body))
            .await
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let status = response.status().as_u16();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !(200..300).contains(&status) {
        let text = String::from_utf8_lossy(&bytes);
        return Err(UpstreamError::UpstreamRequest(format!(
            "deviceauth_usercode_failed: {status} {text}"
        )));
    }
    let parsed = serde_json::from_slice::<DeviceUserCodeResponse>(&bytes)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    Ok((parsed, request_meta))
}

async fn poll_device_authorization(
    client: &WreqClient,
    issuer: &str,
    device_auth_id: &str,
    user_code: &str,
) -> Result<(DeviceAuthPollStatus, UpstreamRequestMeta), UpstreamError> {
    let body = serde_json::to_vec(&json!({
        "device_auth_id": device_auth_id,
        "user_code": user_code,
    }))
    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let url = format!(
        "{}/api/accounts/deviceauth/token",
        issuer.trim_end_matches('/')
    );
    let headers = vec![("content-type".to_string(), "application/json".to_string())];
    let (response, request_meta) =
        tracked_send_request(client, WreqMethod::POST, url.as_str(), headers, Some(body))
            .await
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let status = response.status().as_u16();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if status == 403 || status == 404 {
        return Ok((DeviceAuthPollStatus::Pending, request_meta));
    }
    if !(200..300).contains(&status) {
        let text = String::from_utf8_lossy(&bytes);
        return Err(UpstreamError::UpstreamRequest(format!(
            "deviceauth_poll_failed: {status} {text}"
        )));
    }
    let data = serde_json::from_slice::<DeviceTokenPollResponse>(&bytes)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    if data.authorization_code.trim().is_empty() || data.code_verifier.trim().is_empty() {
        return Err(UpstreamError::UpstreamRequest(
            "deviceauth_poll_failed: missing authorization_code or code_verifier".to_string(),
        ));
    }
    Ok((DeviceAuthPollStatus::Authorized(data), request_meta))
}

async fn exchange_code_for_tokens(
    client: &WreqClient,
    issuer: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
) -> Result<(TokenResponse, UpstreamRequestMeta), UpstreamError> {
    let body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        percent_encode(code),
        percent_encode(redirect_uri),
        percent_encode(CLIENT_ID),
        percent_encode(code_verifier),
    );

    let url = format!("{}/oauth/token", issuer.trim_end_matches('/'));
    let headers = vec![(
        "content-type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    )];
    let (response, request_meta) = tracked_send_request(
        client,
        WreqMethod::POST,
        url.as_str(),
        headers,
        Some(body.into_bytes()),
    )
    .await
    .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let token = parse_token_response("oauth_token_failed", response).await?;
    Ok((token, request_meta))
}

async fn refresh_access_token(
    client: &WreqClient,
    issuer: &str,
    material: &CodexAuthMaterial,
    now_unix_ms: u64,
) -> Result<CodexRefreshedToken, CodexTokenRefreshError> {
    if !material.can_refresh() {
        return Err(CodexTokenRefreshError::InvalidCredential(
            "missing refresh token".to_string(),
        ));
    }

    let body = serde_json::to_vec(&json!({
        "client_id": CLIENT_ID,
        "grant_type": "refresh_token",
        "refresh_token": material.refresh_token,
        "scope": "openid profile email",
    }))
    .map_err(|err| CodexTokenRefreshError::Transient(err.to_string()))?;

    let response = tracked_request(
        client,
        WreqMethod::POST,
        format!("{}/oauth/token", issuer.trim_end_matches('/')).as_str(),
    )
    .header("content-type", "application/json")
    .body(body)
    .send()
    .await
    .map_err(|err| CodexTokenRefreshError::Transient(err.to_string()))?;

    let status = response.status().as_u16();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| CodexTokenRefreshError::Transient(err.to_string()))?;
    let parsed = serde_json::from_slice::<TokenResponse>(&bytes).ok();

    if (200..300).contains(&status) {
        let Some(access_token) = parsed
            .as_ref()
            .and_then(|item| item.access_token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
        else {
            return Err(CodexTokenRefreshError::Transient(
                "oauth token response missing access_token".to_string(),
            ));
        };

        let refresh_token = parsed
            .as_ref()
            .and_then(|item| item.refresh_token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| material.refresh_token.clone());
        if refresh_token.trim().is_empty() {
            return Err(CodexTokenRefreshError::InvalidCredential(
                "oauth token response missing refresh_token".to_string(),
            ));
        }

        let expires_at_unix_ms = now_unix_ms.saturating_add(
            parsed
                .as_ref()
                .and_then(|item| item.expires_in)
                .unwrap_or(3600)
                .saturating_mul(1000),
        );
        let id_token = parsed
            .as_ref()
            .and_then(|item| item.id_token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let effective_id_token = id_token
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(ToString::to_string)
            .or_else(|| (!material.id_token.trim().is_empty()).then(|| material.id_token.clone()));
        let user_email = effective_id_token
            .as_deref()
            .map(parse_id_token_claims)
            .and_then(|claims| claims.email);
        return Ok(CodexRefreshedToken {
            access_token,
            refresh_token,
            expires_at_unix_ms,
            user_email,
            id_token,
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
        format!("refresh_token_failed: status {status}: {payload_text}")
    } else {
        format!("refresh_token_failed: status {status}: {error} {description}")
    };

    if is_invalid_oauth_credential_failure(status, error, description) {
        Err(CodexTokenRefreshError::InvalidCredential(message))
    } else {
        Err(CodexTokenRefreshError::Transient(message))
    }
}

async fn parse_token_response(
    error_prefix: &str,
    response: wreq::Response,
) -> Result<TokenResponse, UpstreamError> {
    let status = response.status().as_u16();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    if !(200..300).contains(&status) {
        let text = String::from_utf8_lossy(&bytes);
        return Err(UpstreamError::UpstreamRequest(format!(
            "{error_prefix}: {status} {text}"
        )));
    }
    serde_json::from_slice::<TokenResponse>(&bytes)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
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
        || joined.contains("refresh_token_expired")
        || joined.contains("refresh_token_reused")
        || joined.contains("refresh_token_invalidated")
}

fn parse_id_token_claims(id_token: &str) -> IdTokenClaims {
    let mut claims = IdTokenClaims::default();
    let mut parts = id_token.split('.');
    let (_header, payload_b64, _signature) = match (parts.next(), parts.next(), parts.next()) {
        (Some(header), Some(payload), Some(signature))
            if !header.is_empty() && !payload.is_empty() && !signature.is_empty() =>
        {
            (header, payload, signature)
        }
        _ => return claims,
    };

    let payload_bytes = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload_b64) {
        Ok(bytes) => bytes,
        Err(_) => return claims,
    };
    let payload = match serde_json::from_slice::<Value>(&payload_bytes) {
        Ok(value) => value,
        Err(_) => return claims,
    };

    let email = payload
        .get("email")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .get("https://api.openai.com/profile")
                .and_then(|profile| profile.get("email"))
                .and_then(Value::as_str)
        })
        .map(ToString::to_string);

    let (plan, account_id) = payload
        .get("https://api.openai.com/auth")
        .map(|auth| {
            let plan = auth
                .get("chatgpt_plan_type")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let account_id = auth
                .get("chatgpt_account_id")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            (plan, account_id)
        })
        .unwrap_or((None, None));

    claims.email = email;
    claims.plan = plan;
    claims.account_id = account_id;
    claims
}

fn normalize_expires_at_ms(value: i64) -> u64 {
    if value <= 0 {
        return 0;
    }
    value as u64
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn default_poll_interval_secs() -> u64 {
    5
}

fn percent_encode(value: &str) -> String {
    form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::{parse_id_token_claims, resolve_manual_code_and_state};

    #[test]
    fn manual_code_parse_prefers_query_code() {
        let (code, state) = resolve_manual_code_and_state(Some(
            "code=direct&state=s1&callback_url=http%3A%2F%2Flocalhost%2Fcb%3Fcode%3Dother%26state%3Ds2",
        ))
        .expect("parse should succeed");
        assert_eq!(code, "direct");
        assert_eq!(state.as_deref(), Some("s1"));
    }

    #[test]
    fn id_token_claim_parse_tolerates_invalid_token() {
        let claims = parse_id_token_claims("invalid");
        assert!(claims.account_id.is_none());
        assert!(claims.email.is_none());
    }
}
