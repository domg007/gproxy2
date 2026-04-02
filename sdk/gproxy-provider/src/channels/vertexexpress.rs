use serde::{Deserialize, Serialize};

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::count_tokens::CountStrategy;
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};

/// Vertex AI Express (API-key-based Vertex AI access) channel.
pub struct VertexExpressChannel;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VertexExpressSettings {
    #[serde(default = "default_vertexexpress_base_url")]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_on_429: Option<u32>,
}

fn default_vertexexpress_base_url() -> String {
    "https://aiplatform.googleapis.com".to_string()
}

impl ChannelSettings for VertexExpressSettings {
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
pub struct VertexExpressCredential {
    pub api_key: String,
}

impl ChannelCredential for VertexExpressCredential {}

impl Channel for VertexExpressChannel {
    const ID: &'static str = "vertexexpress";
    type Settings = VertexExpressSettings;
    type Credential = VertexExpressCredential;
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

            // WebSocket -> stream
            xform("openai_response_websocket", "openai", "stream_generate_content", "gemini"),
            xform("gemini_live", "gemini", "stream_generate_content", "gemini"),

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
        let separator = if request.path.contains('?') { "&" } else { "?" };
        let url = format!(
            "{}{}{}key={}",
            settings.base_url(),
            request.path,
            separator,
            credential.api_key
        );

        let mut builder = http::Request::builder()
            .method(request.method.clone())
            .uri(&url)
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

    fn normalize_response(&self, body: Vec<u8>) -> Vec<u8> {
        crate::utils::vertex_normalize::normalize_vertex_response(body)
    }

    fn count_strategy(&self) -> CountStrategy {
        CountStrategy::UpstreamApi
    }
}

fn vertexexpress_dispatch_table() -> DispatchTable {
    VertexExpressChannel.dispatch_table()
}

inventory::submit! { ChannelRegistration::new(VertexExpressChannel::ID, vertexexpress_dispatch_table) }
