//! Shared OAuth helpers for the credential channels (PKCE, token exchange,
//! refresh). Channel-specific config (client_id, endpoints, scopes) lives in
//! each channel; this is the mechanical PKCE math + form-POST token exchange.
//!
//! Compiled on BOTH native and wasm: the edge build also refreshes OAuth
//! credentials. Randomness comes from `chacha20poly1305`'s `OsRng` (the same
//! source `crypto::envelope` seeds DEKs with — resolves getrandom's js backend
//! on wasm); the PKCE challenge is SHA-256 per the OAuth spec (RFC 7636).

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::channel::ChannelError;
use crate::http::client::UpstreamClient;
use crate::util::rand;

/// Generate a PKCE `(verifier, challenge)` pair (RFC 7636, S256). The verifier
/// is base64url(32 random bytes) → 43 chars (within the 43–128 spec range); the
/// challenge is base64url_nopad(SHA-256(verifier)).
pub fn pkce() -> (String, String) {
    let bytes = rand::bytes::<32>();
    let verifier = B64URL.encode(bytes);
    let challenge = B64URL.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// OAuth token endpoint response. Tolerant: unknown fields are ignored, and
/// every field is optional so a refresh that omits `refresh_token` (the common
/// case — the existing one is reused) still parses.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
}

/// POST `application/x-www-form-urlencoded` `form` pairs to `token_url` and
/// parse the JSON [`TokenResponse`]. Uses the passed [`UpstreamClient`] so the
/// call rides the proxy pool / edge transport. `extra_headers` are appended
/// (e.g. a `User-Agent` some providers require). Non-2xx → [`ChannelError::Build`]
/// carrying the status + (truncated) body.
pub async fn token_post(
    client: &Arc<dyn UpstreamClient>,
    token_url: &str,
    form: &[(&str, &str)],
    extra_headers: &[(&str, &str)],
) -> Result<TokenResponse, ChannelError> {
    let body = encode_form(form);
    let mut builder = http::Request::builder()
        .method(http::Method::POST)
        .uri(token_url)
        .header(
            http::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .header(http::header::ACCEPT, "application/json");
    for (k, v) in extra_headers {
        builder = builder.header(*k, *v);
    }
    let req = builder
        .body(bytes::Bytes::from(body))
        .map_err(|e| ChannelError::Build(format!("token request build: {e}")))?;

    let resp = client
        .send(req)
        .await
        .map_err(|e| ChannelError::Build(format!("token request failed: {e}")))?;
    let (parts, body) = resp.into_parts();
    if !parts.status.is_success() {
        let snippet = String::from_utf8_lossy(&body);
        let snippet: String = snippet.chars().take(256).collect();
        return Err(ChannelError::Build(format!(
            "token endpoint {}: {snippet}",
            parts.status
        )));
    }
    serde_json::from_slice(&body)
        .map_err(|e| ChannelError::Build(format!("token response parse: {e}")))
}

/// Encode `key=value` pairs as `application/x-www-form-urlencoded`. Both keys
/// and values are percent-encoded (RFC 3986 unreserved set kept verbatim).
fn encode_form(pairs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (k, v) in pairs {
        if !out.is_empty() {
            out.push('&');
        }
        percent_encode_into(k, &mut out);
        out.push('=');
        percent_encode_into(v, &mut out);
    }
    out
}

/// Build a Google OAuth2 authorize URL for an authcode+PKCE login, shared by the
/// `geminicli` and `antigravity` channels (same `accounts.google.com` endpoint,
/// differing only in client_id / scope / redirect_uri). Values are
/// percent-encoded. `access_type=offline` + `prompt=consent` ensure a
/// refresh_token is minted (mined from v1).
pub fn google_authorize_url(
    authorize_url: &str,
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    state: &str,
    challenge: &str,
) -> String {
    let query = [
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", scope),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("code_challenge_method", "S256"),
        ("code_challenge", challenge),
        ("state", state),
    ];
    let mut out = String::new();
    for (k, v) in query {
        out.push(if out.is_empty() { '?' } else { '&' });
        percent_encode_into(k, &mut out);
        out.push('=');
        percent_encode_into(v, &mut out);
    }
    format!("{authorize_url}{out}")
}

/// Exchange a Google authcode (+PKCE verifier) for the plaintext secret
/// `{access_token, refresh_token?, expires_at_ms}`, shared by `geminicli` and
/// `antigravity`. NOTE: `project_id` is NOT obtained here — Code Assist project
/// resolution (`loadCodeAssist` / `onboardUser`) is a separate step; the minted
/// secret carries tokens but no `project_id`, which the operator sets later (or
/// a follow-up adds resolution) before the channel can address the API.
pub async fn google_authcode_exchange(
    client: &Arc<dyn UpstreamClient>,
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<serde_json::Value, ChannelError> {
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code_verifier", verifier),
    ];
    let resp = token_post(client, token_url, &form, &[]).await?;

    let access_token = resp
        .access_token
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ChannelError::Build("token response missing access_token".into()))?;
    let expires_at_ms = crate::util::time::unix_now().saturating_mul(1000)
        + resp.expires_in.unwrap_or(3600) as i64 * 1000;

    let mut secret = serde_json::json!({
        "access_token": access_token,
        "expires_at_ms": expires_at_ms,
    });
    if let Some(rt) = resp.refresh_token.filter(|s| !s.is_empty()) {
        secret["refresh_token"] = serde_json::Value::String(rt);
    }
    Ok(secret)
}

/// Percent-encode `s` into `out`, leaving the RFC 3986 unreserved characters
/// (`A-Za-z0-9-._~`) as-is and `%XX`-encoding every other byte.
fn percent_encode_into(s: &str, out: &mut String) {
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(
                char::from_digit((b >> 4) as u32, 16)
                    .unwrap()
                    .to_ascii_uppercase(),
            );
            out.push(
                char::from_digit((b & 0xf) as u32, 16)
                    .unwrap()
                    .to_ascii_uppercase(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PKCE challenge is exactly base64url_nopad(SHA-256(verifier)).
    #[test]
    fn pkce_challenge() {
        let (verifier, challenge) = pkce();
        let expected = B64URL.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, expected);
        // verifier within the RFC 7636 length range, base64url-safe alphabet.
        assert!((43..=128).contains(&verifier.len()));
        assert!(
            verifier
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        );
    }
}
