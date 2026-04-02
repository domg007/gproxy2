use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::channel::{Channel, ChannelCredential, ChannelSettings};
use crate::count_tokens::CountStrategy;
use crate::dispatch::{DispatchTable, RouteImplementation, RouteKey};
use crate::health::ModelCooldownHealth;
use crate::registry::ChannelRegistration;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};
use crate::utils::oauth2_refresh;

/// Codex CLI channel (OpenAI Responses API with OAuth).
pub struct CodexChannel;

const DEFAULT_CODEX_ORIGINATOR: &str = "codex_cli_rs";
const DEFAULT_CODEX_VERSION: &str = "0.118.0";
const DEFAULT_CODEX_OS_TYPE: &str = "Linux";
const DEFAULT_CODEX_OS_VERSION: &str = "6.6";
const DEFAULT_CODEX_ARCH: &str = "x86_64";
const CODEX_SESSION_NAMESPACE: uuid::Uuid = uuid::uuid!("aef2ff08-4585-5e42-a831-1cb53cb6ea8d");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSettings {
    #[serde(default = "default_codex_base_url")]
    pub base_url: String,
    /// Explicit override for the entire User-Agent header.
    /// When set, this takes priority over the computed Codex UA string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_on_429: Option<u32>,
}

impl Default for CodexSettings {
    fn default() -> Self {
        Self {
            base_url: default_codex_base_url(),
            user_agent: None,
            max_retries_on_429: None,
        }
    }
}

fn default_codex_base_url() -> String {
    "https://chatgpt.com/backend-api/codex".to_string()
}

impl CodexSettings {
    /// Build the default Codex CLI user-agent string.
    fn computed_user_agent(&self) -> String {
        let prefix = format!(
            "{}/{} ({} {}; {})",
            DEFAULT_CODEX_ORIGINATOR,
            DEFAULT_CODEX_VERSION,
            DEFAULT_CODEX_OS_TYPE,
            DEFAULT_CODEX_OS_VERSION,
            DEFAULT_CODEX_ARCH
        );
        let terminal_token = codex_terminal_user_agent();
        if terminal_token.is_empty() {
            prefix
        } else {
            format!("{prefix} {terminal_token}")
        }
    }

    /// Return the effective User-Agent: explicit override wins, otherwise computed.
    fn effective_user_agent(&self) -> String {
        match &self.user_agent {
            Some(ua) => ua.clone(),
            None => self.computed_user_agent(),
        }
    }
}

fn is_codex_managed_header(name: &http::HeaderName) -> bool {
    matches!(name.as_str(), "x-client-request-id" | "session_id")
}

fn is_codex_user_agent(request: &PreparedRequest) -> bool {
    request
        .headers
        .get(http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("codex"))
}

fn normalize_codex_model_entry(model: &Value) -> Option<Value> {
    let id = model
        .get("slug")
        .or_else(|| model.get("id"))
        .and_then(Value::as_str)?
        .to_string();

    Some(json!({
        "id": id,
        "created": 0,
        "object": "model",
        "owned_by": "openai"
    }))
}

fn normalize_codex_model_list_response(body: Vec<u8>) -> Vec<u8> {
    let Ok(value) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };

    let Some(models) = value.get("models").and_then(Value::as_array) else {
        return body;
    };

    let normalized_models: Vec<Value> = models
        .iter()
        .filter_map(normalize_codex_model_entry)
        .collect();

    serde_json::to_vec(&json!({
        "object": "list",
        "data": normalized_models,
    }))
    .unwrap_or(body)
}

fn requested_codex_model_id(request: &PreparedRequest) -> Option<String> {
    let body = serde_json::from_slice::<Value>(&request.body).ok()?;
    body.get("path")
        .and_then(|path| path.get("model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_codex_model_get_response(request: &PreparedRequest, body: Vec<u8>) -> Vec<u8> {
    let Ok(value) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };

    if let Some(model) = normalize_codex_model_entry(&value) {
        return serde_json::to_vec(&model).unwrap_or(body);
    }

    let Some(models) = value.get("models").and_then(Value::as_array) else {
        return body;
    };

    let requested_id = requested_codex_model_id(request);
    let selected = requested_id.as_deref().and_then(|target| {
        models.iter().find(|model| {
            model
                .get("slug")
                .or_else(|| model.get("id"))
                .and_then(Value::as_str)
                .is_some_and(|id| id == target)
        })
    });

    let selected = selected.or_else(|| models.first());
    let Some(model) = selected.and_then(normalize_codex_model_entry) else {
        return body;
    };

    serde_json::to_vec(&model).unwrap_or(body)
}

fn codex_terminal_user_agent() -> String {
    let token = if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        let version = std::env::var("TERM_PROGRAM_VERSION").ok();
        match version.as_deref().filter(|v| !v.is_empty()) {
            Some(version) => format!("{term_program}/{version}"),
            None => term_program,
        }
    } else if let Ok(term) = std::env::var("TERM") {
        term
    } else {
        "unknown".to_string()
    };

    token
        .chars()
        .map(|ch| if matches!(ch, ' '..='~') { ch } else { '_' })
        .collect::<String>()
        .trim()
        .to_string()
}

fn request_session_id(request: &PreparedRequest) -> String {
    if let Some(session_id) = request
        .headers
        .get("session_id")
        .or_else(|| request.headers.get("x-client-request-id"))
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
    {
        return session_id.to_owned();
    }

    let body = serde_json::from_slice::<Value>(&request.body).unwrap_or(Value::Null);
    let session_seed = format!(
        "{}\n{}\n{}",
        codex_instructions_fingerprint(&body),
        codex_first_input_fingerprint(&body),
        request.path
    );
    Uuid::new_v5(&CODEX_SESSION_NAMESPACE, session_seed.as_bytes()).to_string()
}

fn codex_instructions_fingerprint(body: &Value) -> String {
    body.get("instructions")
        .map(json_fingerprint_text)
        .unwrap_or_default()
}

fn codex_first_input_fingerprint(body: &Value) -> String {
    match body.get("input") {
        Some(Value::Array(items)) => items.first().map(json_fingerprint_text).unwrap_or_default(),
        Some(value) => json_fingerprint_text(value),
        None => String::new(),
    }
}

fn json_fingerprint_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn normalize_codex_request_body(body: &[u8], is_stream: bool) -> Vec<u8> {
    let Ok(mut body_json) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    let Some(map) = body_json.as_object_mut() else {
        return body.to_vec();
    };

    map.insert("store".to_string(), Value::Bool(false));
    map.remove("max_output_tokens");
    map.remove("metadata");
    map.remove("stream_options");
    map.remove("temperature");
    map.remove("top_p");
    map.remove("top_logprobs");
    map.remove("safety_identifier");
    map.remove("truncation");
    map.insert("stream".to_string(), Value::Bool(is_stream));

    if map
        .get("instructions")
        .is_some_and(|value| !value.is_string())
    {
        map.insert("instructions".to_string(), Value::String(String::new()));
    }

    if !map.contains_key("instructions") {
        map.insert("instructions".to_string(), Value::String(String::new()));
    }

    if let Some(input) = map.get("input")
        && let Some(text) = input.as_str()
    {
        map.insert(
            "input".to_string(),
            json!([
                {
                    "type": "message",
                    "role": "user",
                    "content": text,
                }
            ]),
        );
    }

    serde_json::to_vec(&body_json).unwrap_or_else(|_| body.to_vec())
}

impl ChannelSettings for CodexSettings {
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
pub struct CodexCredential {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default)]
    pub expires_at_ms: u64,
}

impl ChannelCredential for CodexCredential {
    fn apply_update(&mut self, update: &serde_json::Value) -> bool {
        if let Some(token) = update.get("access_token").and_then(|v| v.as_str()) {
            self.access_token = token.to_string();
            if let Some(exp) = update.get("expires_at_ms").and_then(|v| v.as_u64()) {
                self.expires_at_ms = exp;
            }
            if let Some(rt) = update.get("refresh_token").and_then(|v| v.as_str()) {
                self.refresh_token = Some(rt.to_string());
            }
            true
        } else {
            false
        }
    }
}

impl Channel for CodexChannel {
    const ID: &'static str = "codex";
    type Settings = CodexSettings;
    type Credential = CodexCredential;
    type Health = ModelCooldownHealth;

    fn dispatch_table(&self) -> DispatchTable {
        // Native Codex traffic uses the Responses API, but the proxy can still
        // transform other request protocols into openai_response.
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

        let routes: Vec<(RouteKey, RouteImplementation)> = vec![
            // Model list/get
            pass("model_list", "openai"),
            xform("model_list", "claude", "model_list", "openai"),
            xform("model_list", "gemini", "model_list", "openai"),
            pass("model_get", "openai"),
            xform("model_get", "claude", "model_get", "openai"),
            xform("model_get", "gemini", "model_get", "openai"),
            // === No count_tokens routes — uses CountStrategy::Local ===

            // Generate content (internally force stream, then aggregate back)
            xform(
                "generate_content",
                "openai_response",
                "stream_generate_content",
                "openai_response",
            ),
            xform(
                "generate_content",
                "openai_chat_completions",
                "stream_generate_content",
                "openai_response",
            ),
            xform(
                "generate_content",
                "claude",
                "stream_generate_content",
                "openai_response",
            ),
            xform(
                "generate_content",
                "gemini",
                "stream_generate_content",
                "openai_response",
            ),
            // Generate content (stream)
            pass("stream_generate_content", "openai_response"),
            xform(
                "stream_generate_content",
                "openai_chat_completions",
                "stream_generate_content",
                "openai_response",
            ),
            xform(
                "stream_generate_content",
                "claude",
                "stream_generate_content",
                "openai_response",
            ),
            xform(
                "stream_generate_content",
                "gemini",
                "stream_generate_content",
                "openai_response",
            ),
            xform(
                "stream_generate_content",
                "gemini_ndjson",
                "stream_generate_content",
                "openai_response",
            ),
            // WebSocket
            pass("openai_response_websocket", "openai"),
            xform(
                "gemini_live",
                "gemini",
                "stream_generate_content",
                "openai_response",
            ),
            // Images
            pass("create_image", "openai"),
            pass("stream_create_image", "openai"),
            pass("create_image_edit", "openai"),
            pass("stream_create_image_edit", "openai"),
            // Embeddings
            pass("embeddings", "openai"),
            xform("embeddings", "gemini", "embeddings", "openai"),
            // Compact
            pass("compact", "openai"),
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
        let url = format!("{}{}", settings.base_url(), request.path);
        let session_id = request_session_id(request);
        let mut builder = http::Request::builder()
            .method(request.method.clone())
            .uri(&url)
            .header(
                "Authorization",
                format!("Bearer {}", credential.access_token),
            )
            .header("Content-Type", "application/json")
            .header("User-Agent", settings.effective_user_agent())
            .header("originator", DEFAULT_CODEX_ORIGINATOR)
            .header("x-client-request-id", &session_id)
            .header("session_id", &session_id);

        if let Some(account_id) = &credential.account_id
            && !account_id.is_empty()
        {
            builder = builder.header("chatgpt-account-id", account_id.as_str());
        }

        // Forward caller-provided headers (x-codex-turn-state, x-codex-turn-metadata,
        // x-codex-beta-features, OpenAI-Organization, OpenAI-Project, etc.)
        // Keep conversation identity authoritative: upstream expects both
        // x-client-request-id and session_id to equal the same conversation id.
        for (key, value) in request.headers.iter() {
            if is_codex_managed_header(key) {
                continue;
            }
            builder = builder.header(key, value);
        }

        builder
            .body(request.body.clone())
            .map_err(|e| UpstreamError::RequestBuild(e.to_string()))
    }

    fn finalize_request(
        &self,
        _settings: &Self::Settings,
        mut request: PreparedRequest,
    ) -> Result<PreparedRequest, UpstreamError> {
        request.body = match request.path.as_str() {
            "/generate_content" => normalize_codex_request_body(&request.body, false),
            "/stream_generate_content" => normalize_codex_request_body(&request.body, true),
            _ => request.body,
        };
        Ok(request)
    }

    fn normalize_response(&self, request: &PreparedRequest, body: Vec<u8>) -> Vec<u8> {
        match request.path.as_str() {
            "/model_list" if is_codex_user_agent(request) => body,
            "/model_list" => normalize_codex_model_list_response(body),
            "/model_get" => normalize_codex_model_get_response(request, body),
            _ => body,
        }
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
        CountStrategy::Local
    }

    fn refresh_credential<'a>(
        &'a self,
        client: &'a wreq::Client,
        credential: &'a mut Self::Credential,
    ) -> impl std::future::Future<Output = Result<bool, UpstreamError>> + Send + 'a {
        let client = client.clone();
        async move {
            let refresh_token = match &credential.refresh_token {
                Some(rt) if !rt.is_empty() => rt.clone(),
                _ => return Ok(false),
            };
            let result = oauth2_refresh::refresh_oauth2_token(
                &client,
                "https://auth.openai.com/oauth/token",
                "",
                "",
                &refresh_token,
            )
            .await?;
            credential.access_token = result.access_token;
            credential.expires_at_ms = result.expires_at_ms;
            if let Some(rt) = result.refresh_token {
                credential.refresh_token = Some(rt);
            }
            Ok(true)
        }
    }
}
fn codex_dispatch_table() -> DispatchTable {
    CodexChannel.dispatch_table()
}

inventory::submit! { ChannelRegistration::new(CodexChannel::ID, codex_dispatch_table) }
