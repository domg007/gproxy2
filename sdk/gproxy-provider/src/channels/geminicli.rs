use serde::{Deserialize, Serialize};

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::count_tokens::CountStrategy;
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};
use crate::utils::{code_assist_envelope, oauth2_refresh, vertex_normalize};

/// Gemini CLI (Code Assist API) channel with OAuth authentication.
pub struct GeminiCliChannel;

const DEFAULT_GEMINI_CLI_VERSION: &str = "0.35.2";
const DEFAULT_GEMINI_CLI_PLATFORM: &str = "linux";
const DEFAULT_GEMINI_CLI_ARCH: &str = "x64";
const DEFAULT_GEMINI_CLI_SURFACE: &str = "terminal";
const DEFAULT_GOOGLE_GENAI_SDK_VERSION: &str = "1.30.0";
const DEFAULT_GL_NODE_VERSION: &str = "20";

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeminiCliSettings {
    #[serde(default = "default_geminicli_base_url")]
    pub base_url: String,

    /// Explicit user-agent override.  When set, this takes precedence over the
    /// dynamic UA template built from the component fields below.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_on_429: Option<u32>,

    #[serde(default = "default_geminicli_api_version")]
    pub api_version: String,
}

fn default_geminicli_base_url() -> String {
    "https://cloudcode-pa.googleapis.com".to_string()
}

fn default_geminicli_api_version() -> String {
    "v1internal".to_string()
}

impl GeminiCliSettings {
    /// Build the dynamic User-Agent string.
    ///
    /// Template: `GeminiCLI/{version}/{model} ({platform}; {arch}; {surface})`
    fn build_user_agent(&self, model: &str) -> String {
        format!(
            "GeminiCLI/{}/{} ({}; {}; {})",
            DEFAULT_GEMINI_CLI_VERSION,
            model,
            DEFAULT_GEMINI_CLI_PLATFORM,
            DEFAULT_GEMINI_CLI_ARCH,
            DEFAULT_GEMINI_CLI_SURFACE,
        )
    }
}

fn build_x_goog_api_client() -> String {
    format!(
        "google-genai-sdk/{} gl-node/{}",
        DEFAULT_GOOGLE_GENAI_SDK_VERSION, DEFAULT_GL_NODE_VERSION
    )
}

impl ChannelSettings for GeminiCliSettings {
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

// ---------------------------------------------------------------------------
// Credential
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeminiCliCredential {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_at_ms: u64,
    pub project_id: String,
    #[serde(default = "default_geminicli_client_id")]
    pub client_id: String,
    #[serde(default = "default_geminicli_client_secret")]
    pub client_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
}

fn default_geminicli_client_id() -> String {
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com".to_string()
}

fn default_geminicli_client_secret() -> String {
    "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".to_string()
}

impl ChannelCredential for GeminiCliCredential {
    fn apply_update(&mut self, update: &serde_json::Value) -> bool {
        if let Some(token) = update.get("access_token").and_then(|v| v.as_str()) {
            self.access_token = token.to_string();
            if let Some(exp) = update.get("expires_at_ms").and_then(|v| v.as_u64()) {
                self.expires_at_ms = exp;
            }
            if let Some(rt) = update.get("refresh_token").and_then(|v| v.as_str()) {
                self.refresh_token = rt.to_string();
            }
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Channel implementation
// ---------------------------------------------------------------------------

const DEFAULT_MODEL: &str = "gemini-2.5-pro";

impl Channel for GeminiCliChannel {
    const ID: &'static str = "geminicli";
    type Settings = GeminiCliSettings;
    type Credential = GeminiCliCredential;
    type Health = ModelCooldownHealth;

    fn dispatch_table(&self) -> DispatchTable {
        let mut t = DispatchTable::new();
        let pass =
            |op: &str, proto: &str| (RouteKey::new(op, proto), RouteImplementation::Passthrough);
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
            xform(
                "generate_content",
                "openai_chat_completions",
                "generate_content",
                "gemini",
            ),
            xform(
                "generate_content",
                "openai_response",
                "generate_content",
                "gemini",
            ),
            // Generate content (stream)
            pass("stream_generate_content", "gemini"),
            pass("stream_generate_content", "gemini_ndjson"),
            xform(
                "stream_generate_content",
                "claude",
                "stream_generate_content",
                "gemini",
            ),
            xform(
                "stream_generate_content",
                "openai_chat_completions",
                "stream_generate_content",
                "gemini",
            ),
            xform(
                "stream_generate_content",
                "openai_response",
                "stream_generate_content",
                "gemini",
            ),
            // Live API
            pass("gemini_live", "gemini"),
            // WebSocket -> stream
            xform(
                "openai_response_websocket",
                "openai",
                "stream_generate_content",
                "gemini",
            ),
            // Images
            xform("create_image", "openai", "create_image", "gemini"),
            xform(
                "stream_create_image",
                "openai",
                "stream_create_image",
                "gemini",
            ),
            xform("create_image_edit", "openai", "create_image_edit", "gemini"),
            xform(
                "stream_create_image_edit",
                "openai",
                "stream_create_image_edit",
                "gemini",
            ),
            // Embeddings
            pass("embeddings", "gemini"),
            xform("embeddings", "openai", "embeddings", "gemini"),
            // Compact -> generate
            xform("compact", "openai", "generate_content", "gemini"),
        ];

        for (key, imp) in routes {
            t.set(key, imp);
        }
        t
    }

    fn prepare_request(
        &self,
        credential: &Self::Credential,
        settings: &Self::Settings,
        request: &PreparedRequest,
    ) -> Result<http::Request<Vec<u8>>, UpstreamError> {
        // --- body: Code Assist envelope wrapping ---
        let wrapped_body = code_assist_envelope::wrap_request(
            &request.body,
            request.model.as_deref(),
            &credential.project_id,
        )?;

        // --- User-Agent ---
        // If the operator explicitly set `user_agent` in settings, honour that.
        // Otherwise build the dynamic Gemini CLI UA from the component fields.
        let user_agent = match settings.user_agent() {
            Some(ua) => ua.to_string(),
            None => {
                let model = request.model.as_deref().unwrap_or(DEFAULT_MODEL);
                settings.build_user_agent(model)
            }
        };

        let url = format!("{}{}", settings.base_url(), request.path);
        let x_goog_api_client = build_x_goog_api_client();

        let mut builder = http::Request::builder()
            .method(request.method.clone())
            .uri(&url)
            .header(
                "Authorization",
                format!("Bearer {}", credential.access_token),
            )
            .header("Content-Type", "application/json")
            .header("User-Agent", &user_agent)
            .header("x-goog-api-client", x_goog_api_client);

        for (key, value) in request.headers.iter() {
            builder = builder.header(key, value);
        }

        builder
            .body(wrapped_body)
            .map_err(|e| UpstreamError::RequestBuild(e.to_string()))
    }

    fn normalize_response(&self, _request: &PreparedRequest, body: Vec<u8>) -> Vec<u8> {
        let unwrapped = code_assist_envelope::unwrap_response(&body);
        vertex_normalize::normalize_vertex_response(unwrapped)
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
            429 | 499 => {
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

    fn refresh_credential<'a>(
        &'a self,
        client: &'a wreq::Client,
        credential: &'a mut Self::Credential,
    ) -> impl std::future::Future<Output = Result<bool, UpstreamError>> + Send + 'a {
        let client = client.clone();
        async move {
            if credential.refresh_token.is_empty() {
                return Ok(false);
            }
            let result = oauth2_refresh::refresh_oauth2_token(
                &client,
                "https://oauth2.googleapis.com/token",
                &credential.client_id,
                &credential.client_secret,
                &credential.refresh_token,
            )
            .await?;
            credential.access_token = result.access_token;
            credential.expires_at_ms = result.expires_at_ms;
            if let Some(rt) = result.refresh_token {
                credential.refresh_token = rt;
            }
            Ok(true)
        }
    }
}

fn geminicli_dispatch_table() -> DispatchTable {
    GeminiCliChannel.dispatch_table()
}

inventory::submit! { ChannelRegistration::new(GeminiCliChannel::ID, geminicli_dispatch_table) }
