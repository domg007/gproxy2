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
    UpstreamOAuthResponse, UpstreamRequestMeta, tracked_request, tracked_send_request,
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

mod auth;
pub use auth::ensure_geminicli_project_id;
pub(crate) use auth::{geminicli_auth_material_from_credential, resolve_geminicli_access_token};
mod entry;
pub use entry::{execute_geminicli_oauth_callback, execute_geminicli_oauth_start};
mod flow;
use flow::*;
mod token;
use token::*;
