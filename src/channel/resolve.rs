//! Effective proxy + TLS-fingerprint resolution (policy only).
//!
//! Layering:
//! - **proxy**: per-credential override, else the channel default, else the
//!   global default.
//! - **TLS fingerprint**: per-credential override, else the channel's default.
//!
//! These compute the *effective* values; the transport that applies them — a
//! `(proxy, fingerprint)`-keyed upstream-client pool with wreq impersonation —
//! lands in M7. Until then the values are resolved but not yet enforced.

use serde_json::Value;

use crate::channel::Channel;
use crate::store::persistence::records::Credential;

/// Effective outbound proxy: per-credential override, else the channel default,
/// else the global default.
pub fn effective_proxy(
    cred: &Credential,
    channel: &dyn Channel,
    global: Option<&str>,
) -> Option<String> {
    cred.proxy_url
        .clone()
        .or_else(|| channel.default_proxy().map(str::to_string))
        .or_else(|| global.map(str::to_string))
}

/// Effective TLS-emulation fingerprint: per-credential override, else the
/// channel's default.
pub fn effective_tls_fingerprint(cred: &Credential, channel: &dyn Channel) -> Option<Value> {
    cred.tls_fingerprint
        .clone()
        .or_else(|| channel.default_tls_fingerprint())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::bulletins::{claudecode, openai};
    use serde_json::json;

    fn cred(proxy: Option<&str>, tls: Option<Value>) -> Credential {
        Credential {
            id: 1,
            provider_id: 1,
            name: None,
            kind: "api_key".into(),
            secret_json: json!({}),
            weight: 1,
            rpm_limit: None,
            tpm_limit: None,
            proxy_url: proxy.map(str::to_string),
            tls_fingerprint: tls,
            enabled: true,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn proxy_credential_overrides_then_global() {
        let oai = openai::OpenAiChannel;
        // openai has no channel-default proxy → credential override, else global
        let c = cred(Some("http://cred:1"), None);
        assert_eq!(
            effective_proxy(&c, &oai, Some("http://global:2")).as_deref(),
            Some("http://cred:1")
        );
        let c = cred(None, None);
        assert_eq!(
            effective_proxy(&c, &oai, Some("http://global:2")).as_deref(),
            Some("http://global:2")
        );
        assert_eq!(effective_proxy(&cred(None, None), &oai, None), None);
    }

    #[test]
    fn tls_credential_overrides_channel_default() {
        // openai has no default fingerprint → credential override or None
        let c = cred(None, Some(json!({ "profile": "custom" })));
        assert_eq!(
            effective_tls_fingerprint(&c, &openai::OpenAiChannel),
            Some(json!({ "profile": "custom" }))
        );
        assert_eq!(
            effective_tls_fingerprint(&cred(None, None), &openai::OpenAiChannel),
            None
        );
        // claudecode has a default → used when the credential sets none
        assert!(
            effective_tls_fingerprint(&cred(None, None), &claudecode::ClaudeCodeChannel).is_some()
        );
    }
}
