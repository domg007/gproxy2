use serde::{Deserialize, Serialize};

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::count_tokens::CountStrategy;
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};

/// Custom channel — a universal transparent proxy for any OpenAI/Claude/Gemini
/// compatible API endpoint. Forwards requests as-is with configurable auth.
pub struct CustomChannel;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CustomSettings {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_on_429: Option<u32>,
    /// Authentication scheme: "bearer" (default), "x-api-key", "query-key".
    #[serde(default = "default_auth_scheme")]
    pub auth_scheme: String,
}

fn default_auth_scheme() -> String {
    "bearer".to_string()
}

impl ChannelSettings for CustomSettings {
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
pub struct CustomCredential {
    pub api_key: String,
}

impl ChannelCredential for CustomCredential {}

impl Channel for CustomChannel {
    const ID: &'static str = "custom";
    type Settings = CustomSettings;
    type Credential = CustomCredential;
    type Health = ModelCooldownHealth;

    fn dispatch_table(&self) -> DispatchTable {
        let mut t = DispatchTable::new();
        let pass = |op: &str, proto: &str| {
            (RouteKey::new(op, proto), RouteImplementation::Passthrough)
        };

        // Universal passthrough — all protocols supported as-is
        let ops = [
            "model_list",
            "model_get",
            "count_tokens",
            "generate_content",
            "stream_generate_content",
            "embeddings",
            "create_image",
            "stream_create_image",
            "create_image_edit",
            "stream_create_image_edit",
            "compact",
        ];
        let protos = [
            "openai",
            "openai_response",
            "openai_chat_completions",
            "claude",
            "gemini",
            "gemini_ndjson",
        ];

        for op in &ops {
            for proto in &protos {
                t.set(pass(op, proto).0, pass(op, proto).1);
            }
        }

        // WebSocket and Live
        t.set(
            RouteKey::new("openai_response_websocket", "openai"),
            RouteImplementation::Passthrough,
        );
        t.set(
            RouteKey::new("gemini_live", "gemini"),
            RouteImplementation::Passthrough,
        );

        t
    }

    fn prepare_request(
        &self,
        credential: &Self::Credential,
        settings: &Self::Settings,
        request: &PreparedRequest,
    ) -> Result<http::Request<Vec<u8>>, UpstreamError> {
        let url = match settings.auth_scheme.as_str() {
            "query-key" => {
                let sep = if request.path.contains('?') { "&" } else { "?" };
                format!("{}{}{}key={}", settings.base_url(), request.path, sep, credential.api_key)
            }
            _ => format!("{}{}", settings.base_url(), request.path),
        };

        let mut builder = http::Request::builder()
            .method(request.method.clone())
            .uri(&url)
            .header("Content-Type", "application/json");

        match settings.auth_scheme.as_str() {
            "x-api-key" => {
                builder = builder.header("x-api-key", &credential.api_key);
            }
            "query-key" => {
                // Already in URL
            }
            _ => {
                // Default: Bearer
                builder = builder.header("Authorization", format!("Bearer {}", credential.api_key));
            }
        }

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
            529 => ResponseClassification::TransientError,
            500..=599 => ResponseClassification::TransientError,
            _ => ResponseClassification::PermanentError,
        }
    }

    fn count_strategy(&self) -> CountStrategy {
        CountStrategy::Local
    }
}

fn custom_dispatch_table() -> DispatchTable {
    CustomChannel.dispatch_table()
}

inventory::submit! { ChannelRegistration::new(CustomChannel::ID, custom_dispatch_table) }
