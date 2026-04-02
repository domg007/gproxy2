use serde::{Deserialize, Serialize};

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::count_tokens::CountStrategy;
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};

/// Vertex AI (Google Cloud) channel using OAuth2 Bearer token authentication.
///
/// OAuth token refresh is handled externally by the engine/runtime layer.
/// The channel expects `access_token` to be pre-populated on the credential.
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
    /// Current OAuth2 access token (populated by runtime token refresh).
    #[serde(default)]
    pub access_token: String,
}

impl ChannelCredential for VertexCredential {
    fn apply_update(&mut self, update: &serde_json::Value) -> bool {
        if let Some(token) = update.get("access_token").and_then(|v| v.as_str()) {
            self.access_token = token.to_string();
            true
        } else {
            false
        }
    }
}

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
