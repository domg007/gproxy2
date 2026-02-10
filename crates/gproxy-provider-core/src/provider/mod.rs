use async_trait::async_trait;
use bytes::Bytes;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use gproxy_protocol::{claude, gemini, openai};

use crate::headers::{Headers, header_get};
use crate::{
    Credential, DispatchTable, Op, Proto, ProviderConfig, ProviderError, ProviderResult, Request,
    UnavailableReason,
};

pub type ByteStream = tokio::sync::mpsc::Receiver<Bytes>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
        }
    }

    pub fn parse(method: &str) -> Option<Self> {
        if method.eq_ignore_ascii_case("GET") {
            Some(HttpMethod::Get)
        } else if method.eq_ignore_ascii_case("POST") {
            Some(HttpMethod::Post)
        } else if method.eq_ignore_ascii_case("PUT") {
            Some(HttpMethod::Put)
        } else if method.eq_ignore_ascii_case("PATCH") {
            Some(HttpMethod::Patch)
        } else if method.eq_ignore_ascii_case("DELETE") {
            Some(HttpMethod::Delete)
        } else {
            None
        }
    }
}

#[derive(Debug)]
pub enum UpstreamBody {
    Bytes(Bytes),
    Stream(ByteStream),
}

#[derive(Debug)]
pub struct UpstreamHttpResponse {
    pub status: u16,
    pub headers: Headers,
    pub body: UpstreamBody,
}

#[derive(Debug, Clone)]
pub struct UpstreamHttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Headers,
    pub body: Option<Bytes>,
    pub is_stream: bool,
}

/// Downstream request for provider-managed OAuth start.
///
/// This is *not* part of protocol transform; it is a provider internal ability.
#[derive(Debug, Clone)]
pub struct OAuthStartRequest {
    pub query: Option<String>,
    pub headers: Headers,
}

/// Downstream request for provider-managed OAuth callback.
///
/// This is *not* part of protocol transform; it is a provider internal ability.
#[derive(Debug, Clone)]
pub struct OAuthCallbackRequest {
    pub query: Option<String>,
    pub headers: Headers,
}

#[derive(Debug, Clone)]
pub struct OAuthCredential {
    pub name: Option<String>,
    pub settings_json: Option<serde_json::Value>,
    pub credential: Credential,
}

#[derive(Debug)]
pub struct OAuthCallbackResult {
    pub response: UpstreamHttpResponse,
    pub credential: Option<OAuthCredential>,
}

#[derive(Debug, Clone)]
pub struct UpstreamCtx {
    pub trace_id: Option<String>,
    pub user_id: Option<i64>,
    pub user_key_id: Option<i64>,
    pub user_agent: Option<String>,
    pub outbound_proxy: Option<String>,
    pub provider: String,
    pub credential_id: Option<i64>,
    pub op: Op,
    pub internal: bool,
    pub attempt_no: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UpstreamTransportErrorKind {
    Timeout,
    ReadTimeout,
    Connect,
    Dns,
    Tls,
    Other,
}

#[derive(Debug, Clone)]
pub enum UpstreamFailure {
    /// Transport-level failures (no HTTP response).
    Transport {
        kind: UpstreamTransportErrorKind,
        message: String,
    },
    /// HTTP error response captured as bytes (usually non-2xx).
    Http {
        status: u16,
        headers: Headers,
        body: Bytes,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnavailableDecision {
    pub duration: Duration,
    pub reason: UnavailableReason,
}

#[derive(Debug)]
pub enum AuthRetryAction {
    None,
    RetrySame,
    UpdateCredential(Box<Credential>),
}

const RATE_LIMIT_FALLBACK_SECS: u64 = 30;
const SHORT_COOLDOWN_SECS: u64 = 10;
const AUTH_INVALID_YEARS: u64 = 9_999;

pub fn default_decide_unavailable(failure: &UpstreamFailure) -> Option<UnavailableDecision> {
    match failure {
        UpstreamFailure::Http {
            status, headers, ..
        } => {
            if *status == 404 {
                return None;
            }
            if *status == 429 {
                let duration = parse_retry_after(headers)
                    .unwrap_or_else(|| Duration::from_secs(RATE_LIMIT_FALLBACK_SECS));
                return Some(UnavailableDecision {
                    duration,
                    reason: UnavailableReason::RateLimit,
                });
            }
            if *status == 401 || *status == 403 {
                return Some(UnavailableDecision {
                    duration: auth_invalid_duration(),
                    reason: UnavailableReason::AuthInvalid,
                });
            }
            if (500..600).contains(status) {
                return Some(UnavailableDecision {
                    duration: Duration::from_secs(SHORT_COOLDOWN_SECS),
                    reason: UnavailableReason::Upstream5xx,
                });
            }
            None
        }
        UpstreamFailure::Transport { kind, .. } => match kind {
            UpstreamTransportErrorKind::Timeout
            | UpstreamTransportErrorKind::ReadTimeout
            | UpstreamTransportErrorKind::Connect
            | UpstreamTransportErrorKind::Dns
            | UpstreamTransportErrorKind::Tls => Some(UnavailableDecision {
                duration: Duration::from_secs(SHORT_COOLDOWN_SECS),
                reason: UnavailableReason::Timeout,
            }),
            _ => None,
        },
    }
}

fn parse_retry_after(headers: &Headers) -> Option<Duration> {
    let value = header_get(headers, "retry-after")?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let secs = value.parse::<u64>().ok()?;
    Some(Duration::from_secs(secs))
}

fn auth_invalid_duration() -> Duration {
    Duration::from_secs(AUTH_INVALID_YEARS * 365 * 24 * 60 * 60)
}

type ClaudeMessagesRequest = claude::create_message::request::CreateMessageRequest;
type ClaudeCountTokensRequest = claude::count_tokens::request::CountTokensRequest;
type ClaudeModelsListRequest = claude::list_models::request::ListModelsRequest;
type ClaudeModelsGetRequest = claude::get_model::request::GetModelRequest;

type GeminiGenerateContentRequest = gemini::generate_content::request::GenerateContentRequest;
type GeminiStreamGenerateContentRequest =
    gemini::stream_content::request::StreamGenerateContentRequest;
type GeminiCountTokensRequest = gemini::count_tokens::request::CountTokensRequest;
type GeminiModelsListRequest = gemini::list_models::request::ListModelsRequest;
type GeminiModelsGetRequest = gemini::get_model::request::GetModelRequest;

type OpenAIChatCompletionRequest =
    openai::create_chat_completions::request::CreateChatCompletionRequest;
type OpenAIResponseRequest = openai::create_response::request::CreateResponseRequest;
type OpenAIResponseGetRequest = openai::get_response::request::GetResponseRequest;
type OpenAIResponseDeleteRequest = openai::delete_response::request::DeleteResponseRequest;
type OpenAIResponseCancelRequest = openai::cancel_response::request::CancelResponseRequest;
type OpenAIResponseListInputItemsRequest = openai::list_input_items::request::ListInputItemsRequest;
type OpenAIResponseCompactRequest = openai::compact_response::request::CompactResponseRequest;
type OpenAIMemoryTraceSummarizeRequest = openai::trace_summarize::request::TraceSummarizeRequest;
type OpenAIInputTokensRequest = openai::count_tokens::request::InputTokenCountRequest;
type OpenAIModelsListRequest = openai::list_models::request::ListModelsRequest;
type OpenAIModelsGetRequest = openai::get_model::request::GetModelRequest;

#[async_trait]
pub trait UpstreamProvider: Send + Sync {
    fn name(&self) -> &'static str;

    /// Provider "ability table": a dispatch table that tells core whether a given
    /// inbound request shape is handled natively or needs a protocol transform.
    ///
    /// The actual transform execution is performed in core (not provider-impl).
    fn dispatch_table(&self, config: &ProviderConfig) -> DispatchTable;

    // ---- Fine-grained build hooks (per request variant) ----
    // The engine/upstream layer should call these directly after classifying
    // the inbound request into a typed `Request` variant.

    async fn build_claude_messages(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &ClaudeMessagesRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("claude.messages"))
    }

    async fn build_claude_count_tokens(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &ClaudeCountTokensRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("claude.count_tokens"))
    }

    async fn build_claude_models_list(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &ClaudeModelsListRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("claude.models_list"))
    }

    async fn build_claude_models_get(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &ClaudeModelsGetRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("claude.models_get"))
    }

    async fn build_gemini_generate(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &GeminiGenerateContentRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("gemini.generate_content"))
    }

    async fn build_gemini_generate_stream(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &GeminiStreamGenerateContentRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("gemini.stream_generate_content"))
    }

    async fn build_gemini_count_tokens(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &GeminiCountTokensRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("gemini.count_tokens"))
    }

    async fn build_gemini_models_list(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &GeminiModelsListRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("gemini.models_list"))
    }

    async fn build_gemini_models_get(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &GeminiModelsGetRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("gemini.models_get"))
    }

    async fn build_openai_chat(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &OpenAIChatCompletionRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("openai.chat_completions"))
    }

    async fn build_openai_responses(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &OpenAIResponseRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("openai.responses"))
    }

    async fn build_openai_response_get(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &OpenAIResponseGetRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("openai.responses_get"))
    }

    async fn build_openai_response_delete(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &OpenAIResponseDeleteRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("openai.responses_delete"))
    }

    async fn build_openai_response_cancel(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &OpenAIResponseCancelRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("openai.responses_cancel"))
    }

    async fn build_openai_response_list_input_items(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &OpenAIResponseListInputItemsRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported(
            "openai.responses_list_input_items",
        ))
    }

    async fn build_openai_response_compact(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &OpenAIResponseCompactRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("openai.responses_compact"))
    }

    async fn build_openai_memory_trace_summarize(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &OpenAIMemoryTraceSummarizeRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported(
            "openai.memories_trace_summarize",
        ))
    }

    async fn build_openai_input_tokens(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &OpenAIInputTokensRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("openai.input_tokens"))
    }

    async fn build_openai_models_list(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &OpenAIModelsListRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("openai.models_list"))
    }

    async fn build_openai_models_get(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &OpenAIModelsGetRequest,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Unsupported("openai.models_get"))
    }

    /// Provider-managed OAuth start (downstream endpoint).
    ///
    /// Providers that support OAuth (e.g. codex/claudecode/antigravity) should override this.
    fn oauth_start(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _req: &OAuthStartRequest,
    ) -> ProviderResult<UpstreamHttpResponse> {
        Err(ProviderError::Unsupported("oauth_start"))
    }

    /// Provider-managed OAuth callback (downstream endpoint).
    ///
    /// Providers that support OAuth (e.g. codex/claudecode/antigravity) should override this.
    fn oauth_callback(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _req: &OAuthCallbackRequest,
    ) -> ProviderResult<OAuthCallbackResult> {
        Err(ProviderError::Unsupported("oauth_callback"))
    }

    /// Classify an upstream failure into a credential "unavailable" decision.
    ///
    /// This is provider-specific because upstream status codes / error bodies may differ.
    /// Core will call this hook on failures; if it returns `Some`, core should call
    /// `CredentialPool::mark_unavailable(...)`.
    fn decide_unavailable(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &Request,
        _failure: &UpstreamFailure,
    ) -> Option<UnavailableDecision> {
        default_decide_unavailable(_failure)
    }

    fn on_auth_failure<'a>(
        &'a self,
        _ctx: &'a UpstreamCtx,
        _config: &'a ProviderConfig,
        _credential: &'a Credential,
        _req: &'a Request,
        _failure: &'a UpstreamFailure,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<AuthRetryAction>> + Send + 'a>> {
        Box::pin(async { Ok(AuthRetryAction::None) })
    }

    /// Optional hook for non-auth upstream failures.
    ///
    /// Typical use-case: provider-specific fallback decisions (e.g. disable a beta
    /// capability on one credential and retry with downgraded headers).
    fn on_upstream_failure<'a>(
        &'a self,
        _ctx: &'a UpstreamCtx,
        _config: &'a ProviderConfig,
        _credential: &'a Credential,
        _req: &'a Request,
        _failure: &'a UpstreamFailure,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<AuthRetryAction>> + Send + 'a>> {
        Box::pin(async { Ok(AuthRetryAction::None) })
    }

    /// Optional hook for upstream success.
    ///
    /// Typical use-case: persist provider capability learning into credential meta.
    fn on_upstream_success<'a>(
        &'a self,
        _ctx: &'a UpstreamCtx,
        _config: &'a ProviderConfig,
        _credential: &'a Credential,
        _req: &'a Request,
        _response: &'a UpstreamHttpResponse,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<Option<Credential>>> + Send + 'a>> {
        Box::pin(async { Ok(None) })
    }

    /// Optional credential upgrade hook (e.g. exchange session_key for OAuth tokens).
    ///
    /// If this returns `Some(credential)`, core will persist it into the pool and
    /// use the returned credential for the current request.
    fn upgrade_credential<'a>(
        &'a self,
        _ctx: &'a UpstreamCtx,
        _config: &'a ProviderConfig,
        _credential: &'a Credential,
        _req: &'a Request,
    ) -> Pin<Box<dyn Future<Output = ProviderResult<Option<Credential>>> + Send + 'a>> {
        Box::pin(async { Ok(None) })
    }

    /// Optional local response hook for provider-specific endpoints (e.g. local models list/get).
    ///
    /// When this returns `Some`, core should bypass upstream IO and treat the response
    /// as if it were returned from upstream.
    fn local_response(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _req: &Request,
    ) -> ProviderResult<Option<UpstreamHttpResponse>> {
        Ok(None)
    }

    /// Optional non-stream response normalization hook.
    ///
    /// Providers can rewrite upstream JSON body shapes before core decodes
    /// into protocol structs. This is useful for provider-specific REST
    /// envelopes that differ from protocol DTOs.
    #[allow(clippy::too_many_arguments)]
    fn normalize_nonstream_response(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
        _proto: Proto,
        _op: Op,
        _req: &Request,
        body: Bytes,
    ) -> ProviderResult<Bytes> {
        Ok(body)
    }

    async fn build_upstream_usage(
        &self,
        _ctx: &UpstreamCtx,
        _config: &ProviderConfig,
        _credential: &Credential,
    ) -> ProviderResult<UpstreamHttpRequest> {
        Err(ProviderError::Other(
            "upstream_usage not supported by this provider".to_string(),
        ))
    }
}
