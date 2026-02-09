use super::*;
use crate::providers::http_client::{SharedClientKind, client_for_ctx};

#[derive(Debug, serde::Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    exp: i64,
    iat: i64,
}

#[derive(Debug, serde::Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<i64>,
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
        let cfg = match config {
            ProviderConfig::Vertex(cfg) => cfg,
            _ => {
                return Err(ProviderError::InvalidConfig(
                    "expected ProviderConfig::Vertex".to_string(),
                ));
            }
        };
        let token_uri = cfg
            .oauth_token_url
            .as_deref()
            .or(cfg.token_uri.as_deref())
            .unwrap_or(DEFAULT_TOKEN_URI);
        let (token, exp) = fetch_access_token(ctx, credential, token_uri, true)?;
        let mut updated = credential.clone();
        if let Credential::Vertex(sa) = &mut updated {
            sa.access_token = token;
            sa.expires_at = exp;
            return Ok(AuthRetryAction::UpdateCredential(Box::new(updated)));
        }
        Ok(AuthRetryAction::None)
    })
}

pub(super) fn fetch_access_token(
    ctx: &UpstreamCtx,
    credential: &Credential,
    token_uri: &str,
    force_refresh: bool,
) -> ProviderResult<(String, i64)> {
    use jsonwebtoken::{Algorithm, EncodingKey, Header};
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    let sa = match credential {
        Credential::Vertex(sa) => sa,
        _ => {
            return Err(ProviderError::InvalidConfig(
                "expected Credential::Vertex".to_string(),
            ));
        }
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| ProviderError::Other(err.to_string()))?
        .as_secs() as i64;
    if !force_refresh && !sa.access_token.trim().is_empty() && now + 60 < sa.expires_at {
        return Ok((sa.access_token.clone(), sa.expires_at));
    }

    static CACHE: OnceLock<Mutex<HashMap<String, (String, i64)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if !force_refresh {
        let guard = cache
            .lock()
            .map_err(|_| ProviderError::Other("token cache lock failed".to_string()))?;
        if let Some((token, exp)) = guard.get(&sa.client_email)
            && now + 60 < *exp
        {
            return Ok((token.clone(), *exp));
        }
    }

    let exp = now + 3600;
    let claims = JwtClaims {
        iss: &sa.client_email,
        scope: DEFAULT_SCOPE,
        aud: token_uri,
        exp,
        iat: now,
    };
    let mut header = Header::new(Algorithm::RS256);
    if !sa.private_key_id.trim().is_empty() {
        header.kid = Some(sa.private_key_id.clone());
    }
    let key = EncodingKey::from_rsa_pem(sa.private_key.as_bytes())
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let jwt = jsonwebtoken::encode(&header, &claims, &key)
        .map_err(|err| ProviderError::Other(err.to_string()))?;
    let body = format!(
        "grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer&assertion={}",
        urlencoding::encode(&jwt)
    );

    let (access_token, expires_at) = crate::providers::oauth_common::block_on(async {
        let client = client_for_ctx(ctx, SharedClientKind::Global)?;
        let resp = client
            .post(token_uri)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Other(format!(
                "oauth token failed: {} {}",
                status, text
            )));
        }
        let body = resp
            .bytes()
            .await
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        let token_resp: OAuthTokenResponse =
            serde_json::from_slice(&body).map_err(|err| ProviderError::Other(err.to_string()))?;
        Ok::<(String, i64), ProviderError>((
            token_resp.access_token,
            now + token_resp.expires_in.unwrap_or(3600),
        ))
    })?;

    let mut guard = cache
        .lock()
        .map_err(|_| ProviderError::Other("token cache lock failed".to_string()))?;
    guard.insert(sa.client_email.clone(), (access_token.clone(), expires_at));
    Ok((access_token, expires_at))
}
