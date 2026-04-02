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
}

fn default_openai_base_url() -> String {
    "https://api.openai.com".to_string()
}

impl ChannelSettings for OpenAiSettings {
    fn base_url(&self) -> &str {
        &self.base_url
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
        let mut table = DispatchTable::new();
        // OpenAI native routes
        table.set(
            RouteKey::new("generate_content", "openai_chat_completions"),
            RouteImplementation::Passthrough,
        );
        table.set(
            RouteKey::new("generate_content", "openai_response"),
            RouteImplementation::Passthrough,
        );
        table.set(
            RouteKey::new("stream_generate_content", "openai_chat_completions"),
            RouteImplementation::Passthrough,
        );
        table.set(
            RouteKey::new("stream_generate_content", "openai_response"),
            RouteImplementation::Passthrough,
        );
        table.set(
            RouteKey::new("model_list", "openai"),
            RouteImplementation::Passthrough,
        );
        table.set(
            RouteKey::new("model_get", "openai"),
            RouteImplementation::Passthrough,
        );
        table.set(
            RouteKey::new("embeddings", "openai"),
            RouteImplementation::Passthrough,
        );
        table.set(
            RouteKey::new("count_tokens", "openai"),
            RouteImplementation::Passthrough,
        );
        // Cross-protocol routes (Claude → OpenAI, Gemini → OpenAI)
        table.set(
            RouteKey::new("generate_content", "claude"),
            RouteImplementation::TransformTo {
                destination: RouteKey::new("generate_content", "openai_chat_completions"),
            },
        );
        table.set(
            RouteKey::new("generate_content", "gemini"),
            RouteImplementation::TransformTo {
                destination: RouteKey::new("generate_content", "openai_chat_completions"),
            },
        );
        table
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
}

fn openai_dispatch_table() -> DispatchTable {
    OpenAiChannel.dispatch_table()
}

inventory::submit! { ChannelRegistration::new(OpenAiChannel::ID, openai_dispatch_table) }
