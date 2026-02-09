use super::*;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use sha2::Digest;

use crate::providers::oauth_common::{
    extract_code_state_from_callback_url, parse_query_value, resolve_manual_code_and_state,
};
use crate::providers::http_client::{SharedClientKind, client_for_ctx};

const DEFAULT_BROWSER_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OAUTH_SCOPE: &str = "openid profile email offline_access";
const OAUTH_ORIGINATOR: &str = "codex_vscode";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        created_at: Instant,
    },
    AuthorizationCode {
        code_verifier: String,
        redirect_uri: String,
        created_at: Instant,
    },
}

#[derive(Debug, Deserialize)]
struct DeviceUserCodeResponse {
    device_auth_id: String,
    #[serde(alias = "user_code", alias = "usercode")]
    user_code: String,
    #[serde(
        default = "default_poll_interval_secs",
        deserialize_with = "deserialize_poll_interval_secs"
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

static OAUTH_STATES: OnceLock<Mutex<HashMap<String, OAuthState>>> = OnceLock::new();

pub(super) fn oauth_start(
    ctx: &UpstreamCtx,
    _config: &ProviderConfig,
    req: &OAuthStartRequest,
) -> ProviderResult<UpstreamHttpResponse> {
    let mode = parse_oauth_mode(parse_query_value(req.query.as_deref(), "mode").as_deref());
    let state_id = generate_oauth_state();

    let mut guard = oauth_states()
        .lock()
        .map_err(|_| ProviderError::Other("oauth state lock failed".to_string()))?;
    prune_oauth_states(&mut guard);

    match mode {
        OAuthMode::DeviceAuth => {
            let user_code = request_device_user_code(ctx, DEFAULT_ISSUER)?;
            let verification_uri = format!("{}/codex/device", DEFAULT_ISSUER.trim_end_matches('/'));
            guard.insert(
                state_id.clone(),
                OAuthState::DeviceAuth {
                    device_auth_id: user_code.device_auth_id.clone(),
                    user_code: user_code.user_code.clone(),
                    interval_secs: user_code.interval.max(1),
                    created_at: Instant::now(),
                },
            );

            Ok(json_response(serde_json::json!({
                "auth_url": verification_uri,
                "verification_uri": format!("{}/codex/device", DEFAULT_ISSUER.trim_end_matches('/')),
                "user_code": user_code.user_code,
                "interval": user_code.interval.max(1),
                "state": state_id,
                "mode": "device_auth",
                "instructions": "Open verification_uri, enter user_code, then call /oauth/callback with state.",
            })))
        }
        OAuthMode::AuthorizationCode => {
            let code_verifier = generate_code_verifier();
            let code_challenge = generate_code_challenge(&code_verifier);
            let redirect_uri = parse_query_value(req.query.as_deref(), "redirect_uri")
                .unwrap_or_else(|| DEFAULT_BROWSER_REDIRECT_URI.to_string());
            let scope = parse_query_value(req.query.as_deref(), "scope")
                .unwrap_or_else(|| OAUTH_SCOPE.to_string());
            let originator = parse_query_value(req.query.as_deref(), "originator")
                .unwrap_or_else(|| OAUTH_ORIGINATOR.to_string());
            let allowed_workspace_id =
                parse_query_value(req.query.as_deref(), "allowed_workspace_id");
            let auth_url = build_authorize_url(
                DEFAULT_ISSUER,
                &redirect_uri,
                &scope,
                &originator,
                &code_challenge,
                &state_id,
                allowed_workspace_id.as_deref(),
            );

            guard.insert(
                state_id.clone(),
                OAuthState::AuthorizationCode {
                    code_verifier,
                    redirect_uri: redirect_uri.clone(),
                    created_at: Instant::now(),
                },
            );

            Ok(json_response(serde_json::json!({
                "auth_url": auth_url,
                "state": state_id,
                "redirect_uri": redirect_uri,
                "scope": scope,
                "mode": "authorization_code",
                "instructions": "Open auth_url, then call /oauth/callback with code/state (or callback_url).",
            })))
        }
    }
}

pub(super) fn oauth_callback(
    ctx: &UpstreamCtx,
    _config: &ProviderConfig,
    req: &OAuthCallbackRequest,
) -> ProviderResult<OAuthCallbackResult> {
    if let Some(error) = parse_query_value(req.query.as_deref(), "error") {
        let detail = parse_query_value(req.query.as_deref(), "error_description").unwrap_or(error);
        return Ok(OAuthCallbackResult {
            response: json_error(400, &detail),
            credential: None,
        });
    }

    let state_param = parse_query_value(req.query.as_deref(), "state").or_else(|| {
        parse_query_value(req.query.as_deref(), "callback_url")
            .and_then(|url| extract_code_state_from_callback_url(&url).1)
    });
    let (state_id, oauth_state, ambiguous_state) = {
        let mut guard = oauth_states()
            .lock()
            .map_err(|_| ProviderError::Other("oauth state lock failed".to_string()))?;
        prune_oauth_states(&mut guard);
        if let Some(state_id) = state_param.as_deref() {
            (
                Some(state_id.to_string()),
                guard.get(state_id).cloned(),
                false,
            )
        } else if guard.len() == 1 {
            let key = guard.keys().next().cloned();
            (
                key.clone(),
                key.and_then(|state_id| guard.get(&state_id).cloned()),
                false,
            )
        } else {
            (None, None, !guard.is_empty())
        }
    };
    if ambiguous_state {
        return Ok(OAuthCallbackResult {
            response: json_error(400, "ambiguous_state"),
            credential: None,
        });
    }
    let Some(state_id) = state_id else {
        return Ok(OAuthCallbackResult {
            response: json_error(400, "missing state"),
            credential: None,
        });
    };
    let Some(oauth_state) = oauth_state else {
        return Ok(OAuthCallbackResult {
            response: json_error(400, "missing state"),
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
            let poll_status =
                poll_device_authorization(ctx, DEFAULT_ISSUER, &device_auth_id, &user_code)?;
            let poll_success = match poll_status {
                DeviceAuthPollStatus::Pending => {
                    let message = format!(
                        "authorization_pending: retry after {}s",
                        interval_secs.max(1)
                    );
                    return Ok(OAuthCallbackResult {
                        response: json_error(409, &message),
                        credential: None,
                    });
                }
                DeviceAuthPollStatus::Authorized(data) => data,
            };

            {
                let mut guard = oauth_states()
                    .lock()
                    .map_err(|_| ProviderError::Other("oauth state lock failed".to_string()))?;
                guard.remove(&state_id);
            }

            let redirect_uri = format!(
                "{}/deviceauth/callback",
                DEFAULT_ISSUER.trim_end_matches('/')
            );
            let tokens = exchange_code_for_tokens(
                ctx,
                DEFAULT_ISSUER,
                &redirect_uri,
                &poll_success.code_verifier,
                &poll_success.authorization_code,
            )?;
            build_callback_result(tokens)
        }
        OAuthState::AuthorizationCode {
            code_verifier,
            redirect_uri,
            ..
        } => {
            let (code, callback_state) = match resolve_manual_code_and_state(req.query.as_deref()) {
                Ok(value) => value,
                Err(message) => {
                    return Ok(OAuthCallbackResult {
                        response: json_error(400, message),
                        credential: None,
                    });
                }
            };
            if let Some(callback_state) = callback_state
                && callback_state != state_id
            {
                return Ok(OAuthCallbackResult {
                    response: json_error(400, "state_mismatch"),
                    credential: None,
                });
            }

            {
                let mut guard = oauth_states()
                    .lock()
                    .map_err(|_| ProviderError::Other("oauth state lock failed".to_string()))?;
                guard.remove(&state_id);
            }

            let tokens =
                exchange_code_for_tokens(ctx, DEFAULT_ISSUER, &redirect_uri, &code_verifier, &code)?;
            build_callback_result(tokens)
        }
    }
}

pub(super) fn on_auth_failure<'a>(
    ctx: &'a UpstreamCtx,
    _config: &'a ProviderConfig,
    credential: &'a Credential,
    _req: &'a Request,
    _failure: &'a gproxy_provider_core::provider::UpstreamFailure,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderResult<AuthRetryAction>> + Send + 'a>>
{
    Box::pin(async move {
        let refresh_token = match credential {
            Credential::Codex(cred) => cred.refresh_token.clone(),
            _ => return Ok(AuthRetryAction::None),
        };
        let tokens = refresh_access_token(ctx, DEFAULT_ISSUER, &refresh_token).await?;
        let mut updated = credential.clone();
        if let Credential::Codex(cred) = &mut updated {
            cred.access_token = tokens.access_token.clone();
            cred.refresh_token = tokens
                .refresh_token
                .clone()
                .unwrap_or_else(|| cred.refresh_token.clone());
            cred.id_token = tokens
                .id_token
                .clone()
                .unwrap_or_else(|| cred.id_token.clone());
            let email_missing = cred
                .user_email
                .as_ref()
                .map(|value| value.trim().is_empty())
                .unwrap_or(true);
            if email_missing {
                cred.user_email = parse_id_token_claims(&cred.id_token).email;
            }
            return Ok(AuthRetryAction::UpdateCredential(Box::new(updated)));
        }
        Ok(AuthRetryAction::None)
    })
}

pub(super) async fn enrich_credential_profile_if_missing(
    credential: &Credential,
) -> ProviderResult<Option<Credential>> {
    let Credential::Codex(secret) = credential else {
        return Ok(None);
    };
    let email_missing = secret
        .user_email
        .as_ref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true);
    if !email_missing {
        return Ok(None);
    }
    let email = parse_id_token_claims(&secret.id_token).email;
    let Some(email) = email else {
        return Ok(None);
    };
    let mut updated = secret.clone();
    updated.user_email = Some(email);
    Ok(Some(Credential::Codex(updated)))
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
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(&value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}/oauth/authorize?{qs}", issuer.trim_end_matches('/'))
}

fn build_callback_result(tokens: TokenResponse) -> ProviderResult<OAuthCallbackResult> {
    let Some(refresh_token) = tokens.refresh_token.clone() else {
        return Ok(OAuthCallbackResult {
            response: json_error(400, "missing_refresh_token"),
            credential: None,
        });
    };
    let Some(id_token) = tokens.id_token.clone() else {
        return Ok(OAuthCallbackResult {
            response: json_error(400, "missing_id_token"),
            credential: None,
        });
    };

    let claims = tokens
        .id_token
        .as_deref()
        .map(parse_id_token_claims)
        .unwrap_or_default();
    let Some(account_id) = claims.account_id.clone() else {
        return Ok(OAuthCallbackResult {
            response: json_error(400, "missing_account_id"),
            credential: None,
        });
    };

    let credential = OAuthCredential {
        name: claims
            .email
            .clone()
            .or_else(|| Some(format!("codex:{account_id}"))),
        settings_json: None,
        credential: Credential::Codex(CodexCredential {
            access_token: tokens.access_token.clone(),
            refresh_token: refresh_token.clone(),
            id_token: id_token.clone(),
            user_email: claims.email.clone(),
            account_id: account_id.clone(),
            expires_at: 0,
        }),
    };

    Ok(OAuthCallbackResult {
        response: json_response(serde_json::json!({
            "access_token": tokens.access_token,
            "refresh_token": refresh_token,
            "id_token": id_token,
            "account_id": account_id,
            "email": claims.email,
            "plan": claims.plan,
        })),
        credential: Some(credential),
    })
}

fn default_poll_interval_secs() -> u64 {
    5
}

fn deserialize_poll_interval_secs<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    match raw {
        None => Ok(default_poll_interval_secs()),
        Some(serde_json::Value::Number(num)) => num
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("invalid interval number")),
        Some(serde_json::Value::String(value)) => value
            .trim()
            .parse::<u64>()
            .map_err(|err| serde::de::Error::custom(format!("invalid interval: {err}"))),
        Some(_) => Err(serde::de::Error::custom("invalid interval type")),
    }
}

fn request_device_user_code(ctx: &UpstreamCtx, issuer: &str) -> ProviderResult<DeviceUserCodeResponse> {
    crate::providers::oauth_common::block_on(async move {
        let client = client_for_ctx(ctx, SharedClientKind::Global)?;
        let body = serde_json::to_vec(&serde_json::json!({ "client_id": CLIENT_ID }))
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        let resp = client
            .post(format!(
                "{}/api/accounts/deviceauth/usercode",
                issuer.trim_end_matches('/')
            ))
            .header("Content-Type", "application/json")
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
                "deviceauth_usercode_failed: {status} {text}"
            )));
        }
        serde_json::from_slice::<DeviceUserCodeResponse>(&bytes)
            .map_err(|err| ProviderError::Other(err.to_string()))
    })
}

fn poll_device_authorization(
    ctx: &UpstreamCtx,
    issuer: &str,
    device_auth_id: &str,
    user_code: &str,
) -> ProviderResult<DeviceAuthPollStatus> {
    crate::providers::oauth_common::block_on(async move {
        let client = client_for_ctx(ctx, SharedClientKind::Global)?;
        let body = serde_json::to_vec(&serde_json::json!({
            "device_auth_id": device_auth_id,
            "user_code": user_code,
        }))
        .map_err(|err| ProviderError::Other(err.to_string()))?;
        let resp = client
            .post(format!(
                "{}/api/accounts/deviceauth/token",
                issuer.trim_end_matches('/')
            ))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        if status.as_u16() == 403 || status.as_u16() == 404 {
            return Ok(DeviceAuthPollStatus::Pending);
        }
        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes);
            return Err(ProviderError::Other(format!(
                "deviceauth_poll_failed: {status} {text}"
            )));
        }
        let data = serde_json::from_slice::<DeviceTokenPollResponse>(&bytes)
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        if data.authorization_code.trim().is_empty() || data.code_verifier.trim().is_empty() {
            return Err(ProviderError::Other(
                "deviceauth_poll_failed: missing authorization_code or code_verifier".to_string(),
            ));
        }
        Ok(DeviceAuthPollStatus::Authorized(data))
    })
}

fn oauth_states() -> &'static Mutex<HashMap<String, OAuthState>> {
    OAUTH_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prune_oauth_states(states: &mut HashMap<String, OAuthState>) {
    let now = Instant::now();
    states.retain(|_, entry| match entry {
        OAuthState::DeviceAuth { created_at, .. }
        | OAuthState::AuthorizationCode { created_at, .. } => {
            now.duration_since(*created_at) <= Duration::from_secs(OAUTH_STATE_TTL_SECS)
        }
    });
}

fn exchange_code_for_tokens(
    ctx: &UpstreamCtx,
    issuer: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
) -> ProviderResult<TokenResponse> {
    let body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(code),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(CLIENT_ID),
        urlencoding::encode(code_verifier),
    );

    crate::providers::oauth_common::block_on(async move {
        let client = client_for_ctx(ctx, SharedClientKind::Global)?;
        let resp = client
            .post(format!("{}/oauth/token", issuer.trim_end_matches('/')))
            .header("Content-Type", "application/x-www-form-urlencoded")
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
        serde_json::from_slice::<TokenResponse>(&bytes)
            .map_err(|err| ProviderError::Other(err.to_string()))
    })
}

async fn refresh_access_token(
    ctx: &UpstreamCtx,
    issuer: &str,
    refresh_token: &str,
) -> ProviderResult<TokenResponse> {
    let body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}",
        urlencoding::encode(refresh_token),
        urlencoding::encode(CLIENT_ID),
    );
    let client = client_for_ctx(ctx, SharedClientKind::Global)?;
    let resp = client
        .post(format!("{}/oauth/token", issuer.trim_end_matches('/')))
        .header("Content-Type", "application/x-www-form-urlencoded")
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
            "refresh_token_failed: {status} {text}"
        )));
    }
    serde_json::from_slice::<TokenResponse>(&bytes)
        .map_err(|err| ProviderError::Other(err.to_string()))
}
