use super::*;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::providers::http_client::{SharedClientKind, client_for_ctx};
use crate::providers::oauth_common::{parse_query_value, resolve_manual_code_and_state};

#[derive(Debug)]
struct OAuthState {
    code_verifier: String,
    redirect_uri: String,
    created_at: Instant,
}

static OAUTH_STATES: OnceLock<Mutex<HashMap<String, OAuthState>>> = OnceLock::new();

#[derive(Debug, Default)]
struct OAuthProfile {
    email: Option<String>,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

pub(super) fn oauth_start(
    _ctx: &UpstreamCtx,
    config: &ProviderConfig,
    req: &OAuthStartRequest,
) -> ProviderResult<UpstreamHttpResponse> {
    let redirect_uri = parse_query_value(req.query.as_deref(), "redirect_uri")
        .unwrap_or(claudecode_oauth_redirect_uri(config)?);
    let scope =
        parse_query_value(req.query.as_deref(), "scope").unwrap_or_else(|| OAUTH_SCOPE.to_string());

    let (state_id, pkce) = generate_state_and_pkce();
    let claude_ai_base = claudecode_ai_base_url(config)?;
    let auth_url = build_authorize_url(
        claude_ai_base,
        &redirect_uri,
        &pkce.code_challenge,
        &state_id,
        &scope,
    );

    let mut guard = oauth_states()
        .lock()
        .map_err(|_| ProviderError::Other("oauth state lock failed".to_string()))?;
    prune_oauth_states(&mut guard);
    guard.insert(
        state_id.clone(),
        OAuthState {
            code_verifier: pkce.code_verifier,
            redirect_uri: redirect_uri.clone(),
            created_at: Instant::now(),
        },
    );

    Ok(json_response(serde_json::json!({
        "auth_url": auth_url,
        "state": state_id,
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

    let (oauth_state, resolved_state, ambiguous_state) = {
        let mut guard = oauth_states()
            .lock()
            .map_err(|_| ProviderError::Other("oauth state lock failed".to_string()))?;
        prune_oauth_states(&mut guard);
        if let Some(state_id) = state_param.clone() {
            (guard.remove(&state_id), Some(state_id), false)
        } else if guard.len() == 1 {
            let key = guard.keys().next().cloned();
            let state = key.as_ref().and_then(|state_id| guard.remove(state_id));
            (state, key, false)
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
    let Some(oauth_state) = oauth_state else {
        return Ok(OAuthCallbackResult {
            response: json_error(400, "missing state"),
            credential: None,
        });
    };
    let callback_state = state_param.or(resolved_state);

    let api_base = claudecode_api_base_url(config)?;
    let claude_ai_base = claudecode_ai_base_url(config)?;
    let mut tokens = exchange_code_for_tokens(
        ctx,
        api_base,
        claude_ai_base,
        &oauth_state.redirect_uri,
        &oauth_state.code_verifier,
        &code,
        callback_state.as_deref(),
    )?;
    let mut user_email = None;
    if (tokens.subscription_type.is_none() || tokens.rate_limit_tier.is_none())
        && let Ok(profile) = fetch_oauth_profile(ctx, api_base, &tokens.access_token)
    {
        if tokens.subscription_type.is_none() {
            tokens.subscription_type = profile.subscription_type;
        }
        if tokens.rate_limit_tier.is_none() {
            tokens.rate_limit_tier = profile.rate_limit_tier;
        }
        user_email = profile.email;
    }
    let expires_at = tokens.expires_in.map(|v| v + chrono_now()).unwrap_or(0);
    let Some(refresh_token) = tokens.refresh_token.clone() else {
        return Ok(OAuthCallbackResult {
            response: json_error(400, "missing refresh_token"),
            credential: None,
        });
    };
    let subscription_type = tokens.subscription_type.clone();
    let rate_limit_tier = tokens.rate_limit_tier.clone();
    let settings_json = serde_json::json!({
        "subscriptionType": subscription_type,
        "rateLimitTier": rate_limit_tier,
    });
    let settings_json = if settings_json
        .as_object()
        .is_some_and(|obj| obj.values().all(|v| v.is_null()))
    {
        None
    } else {
        Some(settings_json)
    };
    let credential = OAuthCredential {
        name: None,
        settings_json,
        credential: Credential::ClaudeCode(ClaudeCodeCredential {
            access_token: tokens.access_token.clone(),
            refresh_token: refresh_token.clone(),
            expires_at,
            enable_claude_1m_sonnet: None,
            enable_claude_1m_opus: None,
            supports_claude_1m_sonnet: None,
            supports_claude_1m_opus: None,
            subscription_type: tokens.subscription_type.clone().unwrap_or_default(),
            rate_limit_tier: tokens.rate_limit_tier.clone().unwrap_or_default(),
            user_email,
            session_key: None,
        }),
    };

    Ok(OAuthCallbackResult {
        response: json_response(serde_json::json!({
            "access_token": tokens.access_token,
            "refresh_token": tokens.refresh_token,
            "expires_in": tokens.expires_in,
            "subscriptionType": tokens.subscription_type,
            "rateLimitTier": tokens.rate_limit_tier,
        })),
        credential: Some(credential),
    })
}

pub(super) fn on_auth_failure<'a>(
    ctx: &'a UpstreamCtx,
    config: &'a ProviderConfig,
    credential: &'a Credential,
    _req: &'a Request,
    _failure: &'a gproxy_provider_core::provider::UpstreamFailure,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ProviderResult<AuthRetryAction>> + Send + 'a>>
{
    Box::pin(async move {
        match credential {
            Credential::ClaudeCode(secret) => {
                if secret.refresh_token.trim().is_empty() {
                    if let Some(session_key) = secret.session_key.as_deref() {
                        if let Some(cached) = cookie::get_cached_session_tokens_any(session_key) {
                            let api_base = claudecode_api_base_url(config)?;
                            if let Ok(tokens) =
                                refresh_access_token(ctx, api_base, &cached.refresh_token).await
                            {
                                let refreshed = cookie::CachedTokens {
                                    access_token: tokens.access_token.clone(),
                                    refresh_token: tokens
                                        .refresh_token
                                        .clone()
                                        .unwrap_or_else(|| cached.refresh_token.clone()),
                                    expires_at: tokens.expires_in.map(|v| v + chrono_now()),
                                    subscription_type: tokens.subscription_type.clone(),
                                    rate_limit_tier: tokens.rate_limit_tier.clone(),
                                };
                                cookie::cache_session_tokens(session_key, &refreshed);
                                let mut updated_secret = secret.clone();
                                apply_token_profile_from_cached_tokens(
                                    &mut updated_secret,
                                    &refreshed,
                                );
                                updated_secret.access_token = refreshed.access_token.clone();
                                updated_secret.refresh_token = refreshed.refresh_token.clone();
                                updated_secret.expires_at =
                                    refreshed.expires_at.unwrap_or(updated_secret.expires_at);
                                updated_secret.session_key = Some(session_key.to_string());
                                let updated = Credential::ClaudeCode(updated_secret);
                                return Ok(AuthRetryAction::UpdateCredential(Box::new(updated)));
                            }
                        }
                        cookie::clear_session_cache(session_key);
                        return Ok(AuthRetryAction::RetrySame);
                    }
                    return Ok(AuthRetryAction::None);
                };
                let refresh_token = secret.refresh_token.clone();
                let api_base = claudecode_api_base_url(config)?;
                let tokens = refresh_access_token(ctx, api_base, &refresh_token).await?;
                let mut updated = credential.clone();
                if let Credential::ClaudeCode(secret) = &mut updated {
                    secret.access_token = tokens.access_token.clone();
                    if let Some(token) = tokens.refresh_token.clone() {
                        secret.refresh_token = token;
                    }
                    secret.expires_at = tokens
                        .expires_in
                        .map(|v| v + chrono_now())
                        .unwrap_or(secret.expires_at);
                    apply_token_profile_from_token_response(secret, &tokens);
                    return Ok(AuthRetryAction::UpdateCredential(Box::new(updated)));
                }
                Ok(AuthRetryAction::None)
            }
            _ => Ok(AuthRetryAction::None),
        }
    })
}

fn oauth_states() -> &'static Mutex<HashMap<String, OAuthState>> {
    OAUTH_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn prune_oauth_states(states: &mut HashMap<String, OAuthState>) {
    let now = Instant::now();
    states.retain(|_, state| {
        now.duration_since(state.created_at) < Duration::from_secs(OAUTH_STATE_TTL_SECS)
    });
}

fn build_authorize_url(
    claude_ai_base: &str,
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
    scope: &str,
) -> String {
    let qs = format!(
        "code=true&client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        urlencoding::encode(CLIENT_ID),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(scope),
        urlencoding::encode(code_challenge),
        urlencoding::encode(state),
    );
    format!(
        "{}/oauth/authorize?{qs}",
        claude_ai_base.trim_end_matches('/')
    )
}

fn exchange_code_for_tokens(
    ctx: &UpstreamCtx,
    api_base: &str,
    claude_ai_base: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
    state: Option<&str>,
) -> ProviderResult<TokenResponse> {
    let cleaned_code = code.split('#').next().unwrap_or(code);
    let cleaned_code = cleaned_code.split('&').next().unwrap_or(cleaned_code);
    let mut body = format!(
        "grant_type=authorization_code&client_id={}&code={}&redirect_uri={}&code_verifier={}",
        urlencoding::encode(CLIENT_ID),
        urlencoding::encode(cleaned_code),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(code_verifier),
    );
    if let Some(state) = state {
        body.push_str("&state=");
        body.push_str(&urlencoding::encode(state));
    }

    crate::providers::oauth_common::block_on(async move {
        let client = client_for_ctx(ctx, SharedClientKind::ClaudeCode)?;
        let origin = claude_ai_base.trim_end_matches('/');
        let resp = client
            .post(format!("{}/v1/oauth/token", api_base.trim_end_matches('/')))
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
        serde_json::from_slice::<TokenResponse>(&bytes)
            .map_err(|err| ProviderError::Other(err.to_string()))
    })
}

fn parse_oauth_profile(payload: &serde_json::Value) -> OAuthProfile {
    let email = payload
        .get("account")
        .and_then(|value| value.get("email"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    let has_max = payload
        .get("account")
        .and_then(|value| value.get("has_claude_max"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let has_pro = payload
        .get("account")
        .and_then(|value| value.get("has_claude_pro"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let subscription_type = if has_max {
        Some("claude_max".to_string())
    } else if has_pro {
        Some("claude_pro".to_string())
    } else {
        None
    };
    let rate_limit_tier = payload
        .get("organization")
        .and_then(|value| value.get("rate_limit_tier"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    OAuthProfile {
        email,
        subscription_type,
        rate_limit_tier,
    }
}

async fn fetch_oauth_profile_async(
    ctx: &UpstreamCtx,
    api_base: &str,
    access_token: &str,
) -> ProviderResult<OAuthProfile> {
    let client = client_for_ctx(ctx, SharedClientKind::ClaudeCode)?;
    let resp = client
        .get(format!(
            "{}/api/oauth/profile",
            api_base.trim_end_matches('/')
        ))
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", CLAUDE_CODE_UA)
        .header("accept", "application/json")
        .header(HEADER_BETA, OAUTH_BETA)
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
            "oauth_profile_failed: {status} {text}"
        )));
    }
    let payload = serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    Ok(parse_oauth_profile(&payload))
}

fn fetch_oauth_profile(
    ctx: &UpstreamCtx,
    api_base: &str,
    access_token: &str,
) -> ProviderResult<OAuthProfile> {
    crate::providers::oauth_common::block_on(fetch_oauth_profile_async(ctx, api_base, access_token))
}

pub(super) async fn enrich_credential_profile_if_missing(
    ctx: &UpstreamCtx,
    config: &ProviderConfig,
    credential: &Credential,
) -> ProviderResult<Option<Credential>> {
    let Credential::ClaudeCode(secret) = credential else {
        return Ok(None);
    };
    let email_missing = secret
        .user_email
        .as_ref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true);
    let needs_enrich = secret.subscription_type.trim().is_empty()
        || secret.rate_limit_tier.trim().is_empty()
        || email_missing;
    if !needs_enrich || secret.access_token.trim().is_empty() {
        return Ok(None);
    }
    let api_base = claudecode_api_base_url(config)?;
    // Best effort: profile fetch failure should not fail requests.
    let profile = match fetch_oauth_profile_async(ctx, api_base, &secret.access_token).await {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let mut updated = secret.clone();
    let mut changed = false;
    if updated.subscription_type.trim().is_empty()
        && let Some(subscription_type) = profile.subscription_type
    {
        updated.subscription_type = subscription_type;
        changed = true;
    }
    if updated.rate_limit_tier.trim().is_empty()
        && let Some(rate_limit_tier) = profile.rate_limit_tier
    {
        updated.rate_limit_tier = rate_limit_tier;
        changed = true;
    }
    if updated.user_email.is_none()
        && let Some(email) = profile.email
    {
        updated.user_email = Some(email);
        changed = true;
    }
    if !changed {
        return Ok(None);
    }
    Ok(Some(Credential::ClaudeCode(updated)))
}

async fn refresh_access_token(
    ctx: &UpstreamCtx,
    api_base: &str,
    refresh_token: &str,
) -> ProviderResult<TokenResponse> {
    let payload = serde_json::json!({
        "grant_type": "refresh_token",
        "client_id": CLIENT_ID,
        "refresh_token": refresh_token,
    });
    let body = serde_json::to_vec(&payload).map_err(|err| ProviderError::Other(err.to_string()))?;
    let client = client_for_ctx(ctx, SharedClientKind::ClaudeCode)?;
    let resp = client
        .post(format!("{}/v1/oauth/token", api_base.trim_end_matches('/')))
        .header("Content-Type", "application/json")
        .header("User-Agent", TOKEN_UA)
        .header("accept", "application/json, text/plain, */*")
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
