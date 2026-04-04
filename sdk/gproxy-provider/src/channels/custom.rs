use serde::{Deserialize, Serialize};

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::count_tokens::CountStrategy;
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};
use gproxy_protocol::kinds::{OperationFamily, ProtocolKind};

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
        let pass = |op: OperationFamily, proto: ProtocolKind| {
            (RouteKey::new(op, proto), RouteImplementation::Passthrough)
        };

        // Universal passthrough — all protocols supported as-is
        let ops = [
            OperationFamily::ModelList,
            OperationFamily::ModelGet,
            OperationFamily::CountToken,
            OperationFamily::GenerateContent,
            OperationFamily::StreamGenerateContent,
            OperationFamily::Embedding,
            OperationFamily::CreateImage,
            OperationFamily::StreamCreateImage,
            OperationFamily::CreateImageEdit,
            OperationFamily::StreamCreateImageEdit,
            OperationFamily::Compact,
        ];
        let protos = [
            ProtocolKind::OpenAi,
            ProtocolKind::OpenAiResponse,
            ProtocolKind::OpenAiChatCompletion,
            ProtocolKind::Claude,
            ProtocolKind::Gemini,
            ProtocolKind::GeminiNDJson,
        ];

        for &op in &ops {
            for &proto in &protos {
                t.set(pass(op, proto).0, pass(op, proto).1);
            }
        }

        // WebSocket and Live
        t.set(
            RouteKey::new(
                OperationFamily::OpenAiResponseWebSocket,
                ProtocolKind::OpenAi,
            ),
            RouteImplementation::Passthrough,
        );
        t.set(
            RouteKey::new(OperationFamily::GeminiLive, ProtocolKind::Gemini),
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
                format!(
                    "{}{}{}key={}",
                    settings.base_url(),
                    request.path,
                    sep,
                    credential.api_key
                )
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
