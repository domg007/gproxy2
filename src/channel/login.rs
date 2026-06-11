//! OAuth authorization-code login for channels (§14.5).
//!
//! [`Channel`](crate::channel::Channel) covers *per-request* upstream access and
//! the silent `refresh_token` rotation; [`ChannelLogin`] is the orthogonal
//! *first-time* credential acquisition — the interactive authcode + PKCE dance
//! that mints the very secret a channel later refreshes. A channel may impl one,
//! both, or neither (an API-key channel impls neither).
//!
//! Pure async + `serde_json` — no cipher, no persistence, no axum. Compiles on
//! native AND wasm (the registry that holds these is used on every target); the
//! admin HTTP endpoints that drive the flow are native-only. The dual
//! `#[cfg_attr]` async_trait Send/?Send split mirrors [`Channel`].

use std::sync::Arc;

use crate::channel::ChannelError;
use crate::http::client::UpstreamClient;

/// The output of [`ChannelLogin::authcode_start`]: where to send the user, and
/// the redirect_uri the channel actually used.
///
/// `redirect_uri` is echoed back so the matching `complete` exchanges the code
/// with the SAME value the authorize step advertised — OAuth requires them to
/// match. A channel with a fixed redirect_uri ignores the caller's hint and
/// returns its own here.
pub struct AuthCodeStart {
    pub authorize_url: String,
    pub redirect_uri: String,
}

/// Interactive OAuth authorization-code (+PKCE) login for a channel.
///
/// Defaults make the trait opt-in: a channel that does not override returns
/// `None` from [`authcode_start`](ChannelLogin::authcode_start) (no authcode
/// flow) and `Unsupported` from
/// [`authcode_exchange`](ChannelLogin::authcode_exchange).
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait ChannelLogin: Send + Sync {
    /// Build the provider authorize URL for an authcode+PKCE login. `None` means
    /// the channel has no authcode flow. An empty `redirect_uri` tells the
    /// channel to use its own default (returned in [`AuthCodeStart`]).
    fn authcode_start(
        &self,
        _redirect_uri: &str,
        _state: &str,
        _pkce_challenge: &str,
    ) -> Option<AuthCodeStart> {
        None
    }

    /// Exchange an authorization `code` (+ the PKCE `verifier`) for the
    /// PLAINTEXT secret Value. `redirect_uri` MUST equal the one
    /// [`authcode_start`](ChannelLogin::authcode_start) used. The caller seals +
    /// persists the returned Value (purity: the channel never touches
    /// cipher/persistence).
    async fn authcode_exchange(
        &self,
        _client: &Arc<dyn UpstreamClient>,
        _code: &str,
        _verifier: &str,
        _redirect_uri: &str,
    ) -> Result<serde_json::Value, ChannelError> {
        Err(ChannelError::Unsupported("authcode login"))
    }
}
