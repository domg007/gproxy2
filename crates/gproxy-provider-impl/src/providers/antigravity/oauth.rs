use super::*;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::providers::http_client::{SharedClientKind, client_for_ctx};
use crate::providers::oauth_common::{parse_query_value, resolve_manual_code_and_state};

#[derive(Debug)]
struct OAuthState {
    redirect_uri: String,
    created_at: Instant,
    project_id: Option<String>,
    code_verifier: String,
}

static OAUTH_STATES: OnceLock<Mutex<HashMap<String, OAuthState>>> = OnceLock::new();
const MANUAL_REDIRECT_URI: &str = "http://localhost:51121/oauth-callback";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo?alt=json";

pub(super) fn oauth_start(
    _ctx: &UpstreamCtx,
    _config: &ProviderConfig,
    req: &OAuthStartRequest,
) -> ProviderResult<UpstreamHttpResponse> {
    let redirect_uri = parse_query_value(req.query.as_deref(), "redirect_uri")
        .unwrap_or_else(|| MANUAL_REDIRECT_URI.to_string());
    let project_id = parse_query_value(req.query.as_deref(), "project_id");
    let (state, code_verifier, code_challenge) = generate_state_and_pkce();
    let auth_url = build_authorize_url(DEFAULT_AUTH_URL, &redirect_uri, &state, &code_challenge);

    let mut guard = oauth_states()
        .lock()
        .map_err(|_| ProviderError::Other("oauth state lock failed".to_string()))?;
    prune_oauth_states(&mut guard);
    guard.insert(
        state.clone(),
        OAuthState {
            redirect_uri: redirect_uri.clone(),
            created_at: Instant::now(),
            project_id,
            code_verifier,
        },
    );

    Ok(json_response(serde_json::json!({
        "auth_url": auth_url,
        "state": state,
        "redirect_uri": redirect_uri,
        "mode": "manual",
        "instructions": "Open auth_url, then submit code (or callback_url) to /oauth/callback.",
    })))
}

pub(super) fn oauth_callback(
    ctx: &UpstreamCtx,
    config: &ProviderConfig,
    req: &OAuthCallbackRequest,
) -> ProviderResult<OAuthCallbackResult> {
    if let Some(error) = parse_query_value(req.query.as_deref(), "error") {
        let detail = parse_query_value(req.query.as_deref(), "error_description").unwrap_or(error);
        return Ok(OAuthCallbackResult {
            response: json_error(400, &detail),
            credential: None,
        });
    }
    let (code, state_param) = match resolve_manual_code_and_state(req.query.as_deref()) {
        Ok(v) => v,
        Err(msg) => {
            return Ok(OAuthCallbackResult {
                response: json_error(400, msg),
                credential: None,
            });
        }
    };

    let (oauth_state, ambiguous_state) = {
        let mut guard = oauth_states()
            .lock()
            .map_err(|_| ProviderError::Other("oauth state lock failed".to_string()))?;
        prune_oauth_states(&mut guard);
        if let Some(state_id) = state_param.as_deref() {
            (guard.remove(state_id), false)
        } else if guard.len() == 1 {
            let key = guard.keys().next().cloned();
            (key.and_then(|state_id| guard.remove(&state_id)), false)
        } else {
            (None, !guard.is_empty())
        }
    };
    if ambiguous_state {
        return Ok(OAuthCallbackResult {
            response: json_error(400, "ambiguous_state"),
            credential: None,
        });
    }
    let Some(oauth_state) = oauth_state else {
        return Ok(OAuthCallbackResult {
            response: json_error(400, "missing state"),
            credential: None,
        });
    };
    let redirect_uri = oauth_state.redirect_uri;
    let project_id = oauth_state
        .project_id
        .or_else(|| parse_query_value(req.query.as_deref(), "project_id"));

    let tokens = exchange_code_for_tokens(
        ctx,
        &code,
        &redirect_uri,
        &oauth_state.code_verifier,
        DEFAULT_TOKEN_URL,
    )?;
    let Some(refresh_token) = tokens.refresh_token.clone() else {
        return Ok(OAuthCallbackResult {
            response: json_error(400, "missing refresh_token"),
            credential: None,
        });
    };
    let base_url = antigravity_base_url(config)?;
    let project_id = match project_id {
        Some(value) => value,
        None => detect_project_id(ctx, &tokens.access_token, base_url)?
            .unwrap_or_else(random_project_id),
    };
    let user_email = fetch_user_email(ctx, &tokens.access_token).ok().flatten();
    let credential = OAuthCredential {
        name: Some(format!("antigravity:{project_id}")),
        settings_json: None,
        credential: Credential::Antigravity(AntigravityCredential {
            access_token: tokens.access_token.clone(),
            refresh_token: refresh_token.clone(),
            expires_at: tokens
                .expires_in
                .map(|v| v + chrono_now())
                .unwrap_or(chrono_now() + 3600),
            project_id: project_id.clone(),
            client_id: CLIENT_ID.to_string(),
            client_secret: CLIENT_SECRET.to_string(),
            user_email: user_email.clone(),
        }),
    };

    Ok(OAuthCallbackResult {
        response: json_response(serde_json::json!({
            "access_token": tokens.access_token,
            "refresh_token": refresh_token,
            "project_id": project_id,
            "user_email": user_email,
        })),
        credential: Some(credential),
    })
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
            Credential::Antigravity(cred) => cred.refresh_token.clone(),
            _ => return Ok(AuthRetryAction::None),
        };
        let tokens = refresh_access_token(ctx, &refresh_token, DEFAULT_TOKEN_URL).await?;
        let mut updated = credential.clone();
        if let Credential::Antigravity(cred) = &mut updated {
            cred.access_token = tokens.access_token.clone();
            cred.refresh_token = tokens
                .refresh_token
                .clone()
                .unwrap_or_else(|| cred.refresh_token.clone());
            cred.expires_at = tokens
                .expires_in
                .map(|v| v + chrono_now())
                .unwrap_or(cred.expires_at);
            return Ok(AuthRetryAction::UpdateCredential(Box::new(updated)));
        }
        Ok(AuthRetryAction::None)
    })
}

fn oauth_states() -> &'static Mutex<HashMap<String, OAuthState>> {
    OAUTH_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prune_oauth_states(states: &mut HashMap<String, OAuthState>) {
    let now = Instant::now();
    states.retain(|_, entry| {
        now.duration_since(entry.created_at) <= Duration::from_secs(OAUTH_STATE_TTL_SECS)
    });
}

fn generate_state_and_pkce() -> (String, String, String) {
    let mut bytes = [0u8; 32];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut bytes);
    let state = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);

    rng.fill_bytes(&mut bytes);
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
    let params = [
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", OAUTH_SCOPE),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("code_challenge_method", "S256"),
        ("code_challenge", code_challenge),
        ("state", state),
    ];
    let qs = params
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}?{}", auth_url.trim_end_matches('/'), qs)
}

fn exchange_code_for_tokens(
    ctx: &UpstreamCtx,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
    token_url: &str,
) -> ProviderResult<TokenResponse> {
    let body = format!(
        "code={}&client_id={}&client_secret={}&redirect_uri={}&code_verifier={}&grant_type=authorization_code",
        urlencoding::encode(code),
        urlencoding::encode(CLIENT_ID),
        urlencoding::encode(CLIENT_SECRET),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(code_verifier),
    );

    crate::providers::oauth_common::block_on(async move {
        let client = client_for_ctx(ctx, SharedClientKind::Global)?;
        let resp = client
            .post(token_url)
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
    refresh_token: &str,
    token_url: &str,
) -> ProviderResult<TokenResponse> {
    let body = format!(
        "refresh_token={}&client_id={}&client_secret={}&grant_type=refresh_token",
        urlencoding::encode(refresh_token),
        urlencoding::encode(CLIENT_ID),
        urlencoding::encode(CLIENT_SECRET),
    );
    let client = client_for_ctx(ctx, SharedClientKind::Global)?;
    let resp = client
        .post(token_url)
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

fn chrono_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn parse_user_email(payload: &serde_json::Value) -> Option<String> {
    payload
        .get("email")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn fetch_user_email_async(
    ctx: &UpstreamCtx,
    access_token: &str,
) -> ProviderResult<Option<String>> {
    let client = client_for_ctx(ctx, SharedClientKind::Global)?;
    let resp = client
        .get(USERINFO_URL)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", ANTIGRAVITY_USER_AGENT)
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
            "userinfo_failed: {status} {text}"
        )));
    }
    let payload = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    Ok(parse_user_email(&payload))
}

fn fetch_user_email(ctx: &UpstreamCtx, access_token: &str) -> ProviderResult<Option<String>> {
    crate::providers::oauth_common::block_on(fetch_user_email_async(ctx, access_token))
}

pub(super) async fn enrich_credential_profile_if_missing(
    ctx: &UpstreamCtx,
    config: &ProviderConfig,
    credential: &Credential,
) -> ProviderResult<Option<Credential>> {
    let Credential::Antigravity(secret) = credential else {
        return Ok(None);
    };
    let email_missing = secret
        .user_email
        .as_ref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true);
    let project_missing = secret.project_id.trim().is_empty();
    if !email_missing && !project_missing {
        return Ok(None);
    }
    let mut updated = secret.clone();
    let mut changed = false;
    if project_missing {
        let base_url = antigravity_base_url(config)?;
        if let Ok(Some(project_id)) = detect_project_id(ctx, &updated.access_token, base_url)
            && !project_id.trim().is_empty()
        {
            updated.project_id = project_id;
            changed = true;
        }
    }
    if email_missing
        && let Ok(Some(email)) = fetch_user_email_async(ctx, &updated.access_token).await
    {
        updated.user_email = Some(email);
        changed = true;
    }
    if !changed {
        return Ok(None);
    }
    Ok(Some(Credential::Antigravity(updated)))
}
