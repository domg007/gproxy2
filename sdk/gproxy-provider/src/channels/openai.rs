use serde::{Deserialize, Serialize};

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};

/// OpenAI API channel.
pub struct OpenAiChannel;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpenAiSettings {
    #[serde(default = "default_openai_base_url")]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_on_429: Option<u32>,
}

fn default_openai_base_url() -> String {
    "https://api.openai.com".to_string()
}

impl ChannelSettings for OpenAiSettings {
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
pub struct OpenAiCredential {
    pub api_key: String,
}

impl ChannelCredential for OpenAiCredential {}

impl Channel for OpenAiChannel {
    const ID: &'static str = "openai";
    type Settings = OpenAiSettings;
    type Credential = OpenAiCredential;
    type Health = ModelCooldownHealth;

    fn dispatch_table(&self) -> DispatchTable {
        let mut t = DispatchTable::new();

        // Helper: passthrough = src and dst are same
        let pass = |op: &str, proto: &str| {
            (
                RouteKey::new(op, proto),
                RouteImplementation::Passthrough,
            )
        };
        // Helper: transform = src converts to different dst
        let xform = |op: &str, proto: &str, dst_op: &str, dst_proto: &str| {
            (
                RouteKey::new(op, proto),
                RouteImplementation::TransformTo {
                    destination: RouteKey::new(dst_op, dst_proto),
                },
            )
        };

        let routes: Vec<(RouteKey, RouteImplementation)> = vec![
            // === Model list/get ===
            pass("model_list", "openai"),
            xform("model_list", "claude", "model_list", "openai"),
            xform("model_list", "gemini", "model_list", "openai"),
            pass("model_get", "openai"),
            xform("model_get", "claude", "model_get", "openai"),
            xform("model_get", "gemini", "model_get", "openai"),

            // === Count tokens ===
            pass("count_tokens", "openai"),
            xform("count_tokens", "claude", "count_tokens", "openai"),
            xform("count_tokens", "gemini", "count_tokens", "openai"),

            // === Generate content (non-stream) ===
            pass("generate_content", "openai_response"),
            pass("generate_content", "openai_chat_completions"),
            xform("generate_content", "claude", "generate_content", "openai_response"),
            xform("generate_content", "gemini", "generate_content", "openai_response"),

            // === Generate content (stream) ===
            pass("stream_generate_content", "openai_response"),
            pass("stream_generate_content", "openai_chat_completions"),
            xform("stream_generate_content", "claude", "stream_generate_content", "openai_response"),
            xform("stream_generate_content", "gemini", "stream_generate_content", "openai_response"),
            xform("stream_generate_content", "gemini_ndjson", "stream_generate_content", "openai_response"),

            // === WebSocket ===
            pass("openai_response_websocket", "openai"),
            xform("gemini_live", "gemini", "stream_generate_content", "openai_response"),

            // === Images ===
            pass("create_image", "openai"),
            pass("stream_create_image", "openai"),
            pass("create_image_edit", "openai"),
            pass("stream_create_image_edit", "openai"),

            // === Embeddings ===
            pass("embeddings", "openai"),
            xform("embeddings", "gemini", "embeddings", "openai"),

            // === Compact (OpenAI Responses only) ===
            pass("compact", "openai"),
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
            .header("Authorization", format!("Bearer {}", credential.api_key))
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

    fn count_strategy(&self) -> crate::count_tokens::CountStrategy {
        crate::count_tokens::CountStrategy::UpstreamApi
    }
}

fn openai_dispatch_table() -> DispatchTable {
    OpenAiChannel.dispatch_table()
}

inventory::submit! { ChannelRegistration::new(OpenAiChannel::ID, openai_dispatch_table) }
