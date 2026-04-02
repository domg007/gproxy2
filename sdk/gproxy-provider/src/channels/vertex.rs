use std::sync::OnceLock;

use dashmap::DashMap;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::count_tokens::CountStrategy;
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};

const DEFAULT_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const DEFAULT_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

/// Vertex AI (Google Cloud) channel using OAuth2 service account authentication.
///
/// Token refresh is automatic: `refresh_credential` is called before each
/// request and only contacts the token endpoint when the cached token is
/// expired or about to expire.
pub struct VertexChannel;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VertexSettings {
    #[serde(default = "default_vertex_base_url")]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_on_429: Option<u32>,
    #[serde(default = "default_vertex_location")]
    pub location: String,
}

fn default_vertex_base_url() -> String {
    "https://aiplatform.googleapis.com".to_string()
}

fn default_vertex_location() -> String {
    "us-central1".to_string()
}

impl ChannelSettings for VertexSettings {
    fn base_url(&self) -> &str {
        &self.base_url
    }
    fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }
    fn max_retries_on_429(&self) -> u32 {
        self.max_retries_on_429.unwrap_or(3)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VertexCredential {
    /// Google Cloud project ID.
    pub project_id: String,
    /// Service account email.
    pub client_email: String,
    /// PEM-encoded private key for JWT signing.
    pub private_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_uri: Option<String>,
    /// Current OAuth2 access token (populated by token refresh).
    #[serde(default)]
    pub access_token: String,
    /// Token expiry as unix timestamp in milliseconds.
    #[serde(default)]
    pub expires_at_ms: u64,
}

impl ChannelCredential for VertexCredential {
    fn apply_update(&mut self, update: &serde_json::Value) -> bool {
        if let Some(token) = update.get("access_token").and_then(|v| v.as_str()) {
            self.access_token = token.to_string();
            if let Some(exp) = update.get("expires_at_ms").and_then(|v| v.as_u64()) {
                self.expires_at_ms = exp;
            }
            true
        } else {
            false
        }
    }
}

// === Token cache ===

#[derive(Clone)]
struct CachedToken {
    access_token: String,
    expires_at_ms: u64,
}

fn token_cache() -> &'static DashMap<String, CachedToken> {
    static CACHE: OnceLock<DashMap<String, CachedToken>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// === JWT + token exchange ===

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
}

async fn refresh_access_token(
    client: &wreq::Client,
    credential: &VertexCredential,
) -> Result<CachedToken, UpstreamError> {
    let token_uri = credential
        .token_uri
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_TOKEN_URI);

    let now_s = now_ms() / 1000;
    let claims = JwtClaims {
        iss: &credential.client_email,
        scope: DEFAULT_SCOPE,
        aud: token_uri,
        iat: now_s,
        exp: now_s.saturating_add(3600),
    };

    let pem = credential.private_key.replace("\\n", "\n");
    let key = EncodingKey::from_rsa_pem(pem.as_bytes())
        .map_err(|e| UpstreamError::Channel(format!("invalid private key: {e}")))?;

    let mut header = Header::new(Algorithm::RS256);
    header.typ = Some("JWT".to_string());
    let assertion = encode(&header, &claims, &key)
        .map_err(|e| UpstreamError::Channel(format!("jwt sign failed: {e}")))?;

    let body = format!(
        "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={assertion}"
    );

    let resp = client
        .post(token_uri)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| UpstreamError::Http(format!("token refresh: {e}")))?;

    let status = resp.status().as_u16();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| UpstreamError::Http(format!("token refresh body: {e}")))?;

    if !(200..300).contains(&status) {
        let text = String::from_utf8_lossy(&bytes);
        return Err(UpstreamError::Channel(format!(
            "token endpoint status {status}: {text}"
        )));
    }

    let parsed: TokenResponse = serde_json::from_slice(&bytes)
        .map_err(|e| UpstreamError::Channel(format!("token response parse: {e}")))?;

    let access_token = parsed
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| UpstreamError::Channel("token response missing access_token".into()))?;

    let expires_in = parsed.expires_in.unwrap_or(3600);
    let expires_at_ms = now_ms().saturating_add(expires_in.saturating_mul(1000));

    Ok(CachedToken {
        access_token,
        expires_at_ms,
    })
}

// === Channel impl ===

impl Channel for VertexChannel {
    const ID: &'static str = "vertex";
    type Settings = VertexSettings;
    type Credential = VertexCredential;
    type Health = ModelCooldownHealth;

    fn dispatch_table(&self) -> DispatchTable {
        let mut t = DispatchTable::new();
        let pass = |op: &str, proto: &str| {
            (RouteKey::new(op, proto), RouteImplementation::Passthrough)
        };
        let xform = |op: &str, proto: &str, dst_op: &str, dst_proto: &str| {
            (
                RouteKey::new(op, proto),
                RouteImplementation::TransformTo {
                    destination: RouteKey::new(dst_op, dst_proto),
                },
            )
        };

        let routes = vec![
            // Model list/get
            pass("model_list", "gemini"),
            xform("model_list", "claude", "model_list", "gemini"),
            xform("model_list", "openai", "model_list", "gemini"),
            pass("model_get", "gemini"),
            xform("model_get", "claude", "model_get", "gemini"),
            xform("model_get", "openai", "model_get", "gemini"),

            // Count tokens
            pass("count_tokens", "gemini"),
            xform("count_tokens", "claude", "count_tokens", "gemini"),
            xform("count_tokens", "openai", "count_tokens", "gemini"),

            // Generate content (non-stream)
            pass("generate_content", "gemini"),
            xform("generate_content", "claude", "generate_content", "gemini"),
            xform("generate_content", "openai_chat_completions", "generate_content", "gemini"),
            xform("generate_content", "openai_response", "generate_content", "gemini"),

            // Generate content (stream)
            pass("stream_generate_content", "gemini"),
            pass("stream_generate_content", "gemini_ndjson"),
            xform("stream_generate_content", "claude", "stream_generate_content", "gemini"),
            xform("stream_generate_content", "openai_chat_completions", "stream_generate_content", "gemini"),
            xform("stream_generate_content", "openai_response", "stream_generate_content", "gemini"),

            // Live API (native)
            pass("gemini_live", "gemini"),

            // WebSocket -> stream
            xform("openai_response_websocket", "openai", "stream_generate_content", "gemini"),

            // Images
            xform("create_image", "openai", "create_image", "gemini"),
            xform("stream_create_image", "openai", "stream_create_image", "gemini"),
            xform("create_image_edit", "openai", "create_image_edit", "gemini"),
            xform("stream_create_image_edit", "openai", "stream_create_image_edit", "gemini"),

            // Embeddings
            pass("embeddings", "gemini"),
            xform("embeddings", "openai", "embeddings", "gemini"),

            // Compact -> generate
            xform("compact", "openai", "generate_content", "gemini"),
        ];

        for (key, implementation) in routes {
            t.set(key, implementation);
        }
        t
    }

    fn normalize_response(&self, body: Vec<u8>) -> Vec<u8> {
        crate::utils::vertex_normalize::normalize_vertex_response(body)
    }

    fn refresh_credential<'a>(
        &'a self,
        client: &'a wreq::Client,
        credential: &'a mut Self::Credential,
    ) -> impl std::future::Future<Output = Result<bool, UpstreamError>> + Send + 'a {
        let client = client.clone();
        async move {
            // No valid refresh material → can't refresh
            if credential.client_email.is_empty() || credential.private_key.is_empty() {
                return Ok(false);
            }

            // Invalidate any cached token for this email (it just failed)
            token_cache().remove(&credential.client_email);

            // Force refresh
            let token = refresh_access_token(&client, credential).await?;
            credential.access_token = token.access_token.clone();
            credential.expires_at_ms = token.expires_at_ms;
            token_cache().insert(credential.client_email.clone(), token);
            Ok(true)
        }
    }

    fn prepare_request(
        &self,
        credential: &Self::Credential,
        settings: &Self::Settings,
        request: &PreparedRequest,
    ) -> Result<http::Request<Vec<u8>>, UpstreamError> {
        let url = format!("{}{}", settings.base_url(), request.path);
        let mut builder = http::Request::builder()
            .method(request.method.clone())
            .uri(&url)
            .header("Authorization", format!("Bearer {}", credential.access_token))
            .header("Content-Type", "application/json");

        if let Some(ua) = settings.user_agent() {
            builder = builder.header("User-Agent", ua);
        }

        for (key, value) in request.headers.iter() {
            builder = builder.header(key, value);
        }

        builder
            .body(request.body.clone())
            .map_err(|e| UpstreamError::RequestBuild(e.to_string()))
    }

    fn classify_response(
        &self,
        status: u16,
        headers: &http::HeaderMap,
        _body: &[u8],
    ) -> ResponseClassification {
        match status {
            200..=299 => ResponseClassification::Success,
            401 | 403 => ResponseClassification::AuthDead,
            429 => {
                let retry_after = headers
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|secs| secs * 1000);
                ResponseClassification::RateLimited {
                    retry_after_ms: retry_after,
                }
            }
            500..=599 => ResponseClassification::TransientError,
            _ => ResponseClassification::PermanentError,
        }
    }

    fn count_strategy(&self) -> CountStrategy {
        CountStrategy::UpstreamApi
    }
}

fn vertex_dispatch_table() -> DispatchTable {
    VertexChannel.dispatch_table()
}

inventory::submit! { ChannelRegistration::new(VertexChannel::ID, vertex_dispatch_table) }
