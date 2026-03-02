use std::sync::OnceLock;

use dashmap::DashMap;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{DEFAULT_SCOPE, DEFAULT_TOKEN_URI, TOKEN_REFRESH_SKEW_MS};
use super::credential::VertexServiceAccountCredential;
use crate::channels::ChannelSettings;
use crate::channels::upstream::tracked_request;

#[derive(Clone)]
pub(crate) struct VertexAuthMaterial {
    pub(crate) access_token: String,
    pub(crate) expires_at_unix_ms: u64,
    pub(crate) project_id: String,
    client_email: String,
    private_key: String,
    token_uri: String,
}

impl VertexAuthMaterial {
    fn access_token_valid(&self, now_unix_ms: u64) -> bool {
        !self.access_token.trim().is_empty()
            && self
                .expires_at_unix_ms
                .saturating_sub(TOKEN_REFRESH_SKEW_MS)
                > now_unix_ms
    }

    fn can_refresh(&self) -> bool {
        !self.project_id.trim().is_empty()
            && !self.client_email.trim().is_empty()
            && !self.private_key.trim().is_empty()
    }
}

#[derive(Debug, Clone)]
struct CachedVertexToken {
    access_token: String,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct VertexRefreshedToken {
    pub(crate) access_token: String,
    pub(crate) expires_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct VertexResolvedAccessToken {
    pub(crate) access_token: String,
    pub(crate) expires_at_unix_ms: u64,
    pub(crate) refreshed: Option<VertexRefreshedToken>,
}

fn vertex_token_cache() -> &'static DashMap<String, CachedVertexToken> {
    static CACHE: OnceLock<DashMap<String, CachedVertexToken>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum VertexTokenRefreshError {
    #[error("invalid vertex credential: {0}")]
    InvalidCredential(String),
    #[error("transient token refresh error: {0}")]
    Transient(String),
}

impl VertexTokenRefreshError {
    pub(crate) fn as_message(&self) -> String {
        self.to_string()
    }

    pub(crate) fn is_invalid_credential(&self) -> bool {
        matches!(self, Self::InvalidCredential(_))
    }
}

#[derive(Debug, Deserialize)]
struct VertexTokenResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

pub(crate) fn vertex_auth_material_from_credential(
    value: &VertexServiceAccountCredential,
    settings: &ChannelSettings,
) -> Option<VertexAuthMaterial> {
    let token_uri = value
        .token_uri
        .as_deref()
        .map(str::trim)
        .filter(|uri| !uri.is_empty())
        .or_else(|| {
            settings
                .oauth_token_url()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or(DEFAULT_TOKEN_URI)
        .to_string();
    let material = VertexAuthMaterial {
        access_token: value.access_token.trim().to_string(),
        expires_at_unix_ms: normalize_expires_at_ms(value.expires_at),
        project_id: value.project_id.trim().to_string(),
        client_email: value.client_email.trim().to_string(),
        private_key: value.private_key.clone(),
        token_uri,
    };

    if material.project_id.is_empty() {
        None
    } else {
        Some(material)
    }
}

pub(crate) async fn resolve_vertex_access_token(
    client: &WreqClient,
    cache_key: &str,
    material: &VertexAuthMaterial,
    now_unix_ms: u64,
    force_refresh: bool,
) -> Result<VertexResolvedAccessToken, VertexTokenRefreshError> {
    if !force_refresh {
        if let Some(cached) = vertex_token_cache().get(cache_key).filter(|item| {
            item.expires_at_unix_ms
                .saturating_sub(TOKEN_REFRESH_SKEW_MS)
                > now_unix_ms
        }) {
            return Ok(VertexResolvedAccessToken {
                access_token: cached.access_token.clone(),
                expires_at_unix_ms: cached.expires_at_unix_ms,
                refreshed: None,
            });
        }

        if material.access_token_valid(now_unix_ms) {
            vertex_token_cache().insert(
                cache_key.to_string(),
                CachedVertexToken {
                    access_token: material.access_token.clone(),
                    expires_at_unix_ms: material.expires_at_unix_ms,
                },
            );
            return Ok(VertexResolvedAccessToken {
                access_token: material.access_token.clone(),
                expires_at_unix_ms: material.expires_at_unix_ms,
                refreshed: None,
            });
        }
    }

    let refreshed = refresh_vertex_access_token(client, material, now_unix_ms).await?;
    vertex_token_cache().insert(
        cache_key.to_string(),
        CachedVertexToken {
            access_token: refreshed.access_token.clone(),
            expires_at_unix_ms: refreshed.expires_at_unix_ms,
        },
    );
    Ok(VertexResolvedAccessToken {
        access_token: refreshed.access_token.clone(),
        expires_at_unix_ms: refreshed.expires_at_unix_ms,
        refreshed: Some(refreshed),
    })
}

async fn refresh_vertex_access_token(
    client: &WreqClient,
    material: &VertexAuthMaterial,
    now_unix_ms: u64,
) -> Result<VertexRefreshedToken, VertexTokenRefreshError> {
    if !material.can_refresh() {
        return Err(VertexTokenRefreshError::InvalidCredential(
            "missing service-account refresh fields".to_string(),
        ));
    }

    let assertion = build_service_account_jwt_assertion(material, now_unix_ms)?;
    let body = format!(
        "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={assertion}"
    );

    let response = tracked_request(client, WreqMethod::POST, material.token_uri.as_str())
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|err| VertexTokenRefreshError::Transient(err.to_string()))?;

    let status = response.status().as_u16();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| VertexTokenRefreshError::Transient(err.to_string()))?;

    let parsed = serde_json::from_slice::<VertexTokenResponse>(&bytes).ok();
    if (200..300).contains(&status) {
        let access_token = parsed
            .as_ref()
            .and_then(|value| value.access_token.as_deref())
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                VertexTokenRefreshError::Transient(
                    "oauth token response missing access_token".to_string(),
                )
            })?
            .to_string();

        let expires_in = parsed
            .as_ref()
            .and_then(|value| value.expires_in)
            .unwrap_or(3600);
        let expires_at_unix_ms = now_unix_ms.saturating_add(expires_in.saturating_mul(1000));
        return Ok(VertexRefreshedToken {
            access_token,
            expires_at_unix_ms,
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
        Err(VertexTokenRefreshError::InvalidCredential(message))
    } else {
        Err(VertexTokenRefreshError::Transient(message))
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
}

fn build_service_account_jwt_assertion(
    material: &VertexAuthMaterial,
    now_unix_ms: u64,
) -> Result<String, VertexTokenRefreshError> {
    let now_unix_s = now_unix_ms / 1000;
    let claims = JwtClaims {
        iss: material.client_email.as_str(),
        scope: DEFAULT_SCOPE,
        aud: material.token_uri.as_str(),
        iat: now_unix_s,
        exp: now_unix_s.saturating_add(3600),
    };

    let private_key_pem = material.private_key.replace("\\n", "\n");
    let key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes()).map_err(|err| {
        VertexTokenRefreshError::InvalidCredential(format!("invalid private key: {err}"))
    })?;
    let mut header = Header::new(Algorithm::RS256);
    header.typ = Some("JWT".to_string());
    encode(&header, &claims, &key).map_err(|err| {
        VertexTokenRefreshError::InvalidCredential(format!(
            "sign service-account jwt assertion failed: {err}"
        ))
    })
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
