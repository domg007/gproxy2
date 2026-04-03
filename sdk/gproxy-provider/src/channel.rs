use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::dispatch::DispatchTable;
use crate::health::CredentialHealth;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError};

/// Core abstraction for an upstream LLM API provider channel.
///
/// Each channel (OpenAI, Anthropic, Gemini, etc.) implements this trait once.
/// Registration is automatic via `inventory::submit!`.
pub trait Channel: Send + Sync + 'static {
    /// Unique channel identifier (e.g. "openai", "anthropic").
    const ID: &'static str;

    /// Channel-specific configuration.
    type Settings: ChannelSettings;
    /// Channel-specific credential (API key, OAuth tokens, etc.).
    type Credential: ChannelCredential;
    /// Channel-specific health tracking shape.
    type Health: CredentialHealth;

    /// Default dispatch table mapping (operation, protocol) → route strategy.
    fn dispatch_table(&self) -> DispatchTable;

    /// Build an HTTP request from credential + settings + prepared request.
    fn prepare_request(
        &self,
        credential: &Self::Credential,
        settings: &Self::Settings,
        request: &PreparedRequest,
    ) -> Result<http::Request<Vec<u8>>, UpstreamError>;

    /// Finalize the semantic upstream request after protocol transform but
    /// before credential selection and HTTP transport wrapping.
    ///
    /// This is the right place for protocol/body normalization that should be
    /// visible to routing or cache-affinity logic. Transport-specific wrapping
    /// (auth headers, private envelopes, request ids) should remain in
    /// `prepare_request`.
    fn finalize_request(
        &self,
        _settings: &Self::Settings,
        request: PreparedRequest,
    ) -> Result<PreparedRequest, UpstreamError> {
        Ok(request)
    }

    /// Classify an upstream response to decide retry behavior.
    fn classify_response(
        &self,
        status: u16,
        headers: &http::HeaderMap,
        body: &[u8],
    ) -> ResponseClassification;

    /// Normalize the upstream response body (fix non-standard fields, etc.).
    /// Called before usage extraction and protocol transform.
    /// Default: no-op, return body as-is.
    fn normalize_response(&self, _request: &PreparedRequest, body: Vec<u8>) -> Vec<u8> {
        body
    }

    /// Token counting strategy for this channel.
    /// Default: local (tiktoken for GPT, DeepSeek fallback for others).
    fn count_strategy(&self) -> crate::count_tokens::CountStrategy {
        crate::count_tokens::CountStrategy::Local
    }

    /// Handle a local route (no upstream call). Returns None if not supported.
    fn handle_local(
        &self,
        _operation: &str,
        _protocol: &str,
        _body: &[u8],
    ) -> Option<Result<Vec<u8>, UpstreamError>> {
        None
    }

    /// Attempt to refresh a credential after an auth failure (401/403).
    /// Called when upstream returns AuthDead. Returns `true` if the credential
    /// was updated and the request should be retried once more.
    /// Default: no refresh capability, returns `false`.
    fn refresh_credential<'a>(
        &'a self,
        _client: &'a wreq::Client,
        _credential: &'a mut Self::Credential,
    ) -> impl Future<Output = Result<bool, UpstreamError>> + Send + 'a {
        async { Ok(false) }
    }

    /// Start an OAuth flow (optional, most channels return None).
    fn oauth_start<'a>(
        &'a self,
        _client: &'a wreq::Client,
        _settings: &'a Self::Settings,
        _params: &'a BTreeMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = Result<Option<OAuthFlow>, UpstreamError>> + Send + 'a>> {
        Box::pin(async { Ok(None) })
    }

    fn oauth_finish<'a>(
        &'a self,
        _client: &'a wreq::Client,
        _settings: &'a Self::Settings,
        _params: &'a BTreeMap<String, String>,
    ) -> Pin<
        Box<
            dyn Future<
                    Output = Result<Option<OAuthCredentialResult<Self::Credential>>, UpstreamError>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(None) })
    }
}

/// Channel configuration (base URL, user agent, retry, etc.).
pub trait ChannelSettings:
    Send + Sync + Clone + Default + Serialize + DeserializeOwned + 'static
{
    fn base_url(&self) -> &str;
    fn user_agent(&self) -> Option<&str> {
        None
    }
    /// Max retries per credential on 429 without retry-after header.
    fn max_retries_on_429(&self) -> u32 {
        3
    }
}

/// Channel credential (API key, OAuth token, etc.).
pub trait ChannelCredential: Send + Sync + Clone + Serialize + DeserializeOwned + 'static {
    /// Apply an upstream credential update (e.g. OAuth token refresh).
    /// Returns true if the update was applied.
    fn apply_update(&mut self, _update: &serde_json::Value) -> bool {
        false
    }
}

/// Placeholder for OAuth flow data.
#[derive(Debug, Clone)]
pub struct OAuthFlow {
    pub authorize_url: String,
    pub state: String,
    pub redirect_uri: Option<String>,
    pub verification_uri: Option<String>,
    pub user_code: Option<String>,
    pub mode: Option<String>,
    pub scope: Option<String>,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthCredentialResult<C> {
    pub credential: C,
    pub details: Value,
}
