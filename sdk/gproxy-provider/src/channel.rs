use serde::{de::DeserializeOwned, Serialize};

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

    /// Classify an upstream response to decide retry behavior.
    fn classify_response(
        &self,
        status: u16,
        headers: &http::HeaderMap,
        body: &[u8],
    ) -> ResponseClassification;

    /// Start an OAuth flow (optional, most channels return None).
    fn oauth_start(&self) -> Option<OAuthFlow> {
        None
    }
}

/// Channel configuration (base URL, user agent, etc.).
pub trait ChannelSettings: Send + Sync + Clone + Default + Serialize + DeserializeOwned + 'static {
    fn base_url(&self) -> &str;
    fn user_agent(&self) -> Option<&str> {
        None
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
}
