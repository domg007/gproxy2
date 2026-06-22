//! Two-step sentinel challenge flow for chatgpt.com, adapted to the v2
//! [`UpstreamClient`] transport (ported from v1 `channels/chatgpt/sentinel.rs`).
//!
//! Sequence:
//! 1. POST `/sentinel/chat-requirements/prepare` with `{p: <config>}`.
//!    Response: `{prepare_token, proofofwork: {seed, difficulty}, persona}`.
//! 2. Solve PoW locally (pure CPU).
//! 3. POST `/sentinel/chat-requirements/finalize` with `{prepare_token, proofofwork}`.
//!    Response: `{token, persona}` — `token` is the
//!    `openai-sentinel-chat-requirements-token` value; the PoW answer is the
//!    `openai-sentinel-proof-token`.
//!
//! Turnstile is deliberately NOT sent (live testing confirmed finalize accepts
//! the absence). Warmup (`__cf_bm` capture) lives in `cookie.rs`/`auth.rs`.

use std::sync::Arc;

use bytes::Bytes;
use http::{Request, Response};
use serde::Deserialize;
use serde_json::Value;

use super::config::{ConfigOptions, build_prepare_p};
use super::headers::standard_headers;
use super::pow::solve_pow;
use crate::channel::ChannelError;
use crate::http::client::UpstreamClient;

const PREPARE_URL: &str = "https://chatgpt.com/backend-api/sentinel/chat-requirements/prepare";
const FINALIZE_URL: &str = "https://chatgpt.com/backend-api/sentinel/chat-requirements/finalize";

/// Tokens returned by a successful sentinel round.
#[derive(Debug, Clone)]
pub struct SentinelTokens {
    /// Value for `openai-sentinel-chat-requirements-token` header.
    pub chat_req_token: String,
    /// Value for `openai-sentinel-proof-token` header (same PoW answer sent to
    /// finalize).
    pub proof_token: String,
    /// Unix millis at which `chat_req_token` expires (decoded from the JWT `exp`
    /// claim). `0` if it could not be decoded.
    pub chat_req_token_expires_at_ms: i64,
    /// Upstream persona classification (e.g. `chatgpt-paid`, `chatgpt-free`).
    pub persona: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PrepareResponse {
    prepare_token: String,
    #[serde(default)]
    persona: Option<String>,
    proofofwork: Option<ProofOfWorkInfo>,
    // turnstile field is present but intentionally ignored.
}

#[derive(Debug, Deserialize)]
struct ProofOfWorkInfo {
    #[serde(default)]
    required: bool,
    seed: Option<String>,
    difficulty: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FinalizeResponse {
    token: String,
    #[serde(default)]
    persona: Option<String>,
}

/// Run prepare → PoW → finalize and return the tokens. The same `client` should
/// have already warmed up so the `__cf_bm` cookie established during warmup is
/// reused on these calls.
pub async fn run_sentinel(
    client: &Arc<dyn UpstreamClient>,
    access_token: &str,
) -> Result<SentinelTokens, ChannelError> {
    let opts = ConfigOptions::browser_default();
    let p = build_prepare_p(&opts);
    let prep: PrepareResponse = send_json(
        client,
        PREPARE_URL,
        access_token,
        serde_json::json!({ "p": p }),
    )
    .await?;

    let pow_info = prep
        .proofofwork
        .ok_or_else(|| ChannelError::Build("sentinel prepare: missing proofofwork".into()))?;
    let (seed, difficulty) = match (&pow_info.seed, &pow_info.difficulty) {
        (Some(s), Some(d)) => (s.clone(), d.clone()),
        _ if !pow_info.required => (String::new(), String::new()),
        _ => {
            return Err(ChannelError::Build(
                "sentinel prepare: proofofwork required but seed/difficulty missing".into(),
            ));
        }
    };

    let proof_token = if seed.is_empty() {
        String::new()
    } else {
        solve_pow(&seed, &difficulty, &opts)
    };

    let mut fin_body = serde_json::Map::new();
    fin_body.insert("prepare_token".into(), Value::String(prep.prepare_token));
    if !proof_token.is_empty() {
        fin_body.insert("proofofwork".into(), Value::String(proof_token.clone()));
    }
    let fin: FinalizeResponse =
        send_json(client, FINALIZE_URL, access_token, fin_body.into()).await?;

    let expires_at_ms = decode_jwt_exp_ms(&fin.token).unwrap_or(0);
    Ok(SentinelTokens {
        chat_req_token: fin.token,
        proof_token,
        chat_req_token_expires_at_ms: expires_at_ms,
        persona: fin.persona.or(prep.persona),
    })
}

/// POST `body` to `url` with the standard chatgpt header set, decoding the JSON
/// response into `T`.
async fn send_json<T: serde::de::DeserializeOwned>(
    client: &Arc<dyn UpstreamClient>,
    url: &str,
    access_token: &str,
    body: Value,
) -> Result<T, ChannelError> {
    let raw = serde_json::to_vec(&body)
        .map_err(|e| ChannelError::Build(format!("sentinel encode: {e}")))?;
    let mut builder = Request::post(url);
    if let Some(headers) = builder.headers_mut() {
        *headers = standard_headers(access_token);
    }
    let req = builder
        .body(Bytes::from(raw))
        .map_err(|e| ChannelError::Build(format!("sentinel request build: {e}")))?;
    let resp: Response<Bytes> = client
        .send(req)
        .await
        .map_err(|e| ChannelError::Build(format!("sentinel http: {e}")))?;
    let status = resp.status();
    let bytes = resp.into_body();
    if !status.is_success() {
        return Err(ChannelError::Build(format!(
            "sentinel {url} {status}: {}",
            String::from_utf8_lossy(&bytes)
                .chars()
                .take(400)
                .collect::<String>()
        )));
    }
    serde_json::from_slice(&bytes).map_err(|e| ChannelError::Build(format!("sentinel decode: {e}")))
}

/// Decode the `exp` claim from a JWT-shaped token. Returns unix millis.
pub fn decode_jwt_exp_ms(token: &str) -> Option<i64> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    let claims = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let v: Value = serde_json::from_slice(&claims).ok()?;
    let exp = v.get("exp")?.as_i64()?;
    Some(exp.saturating_mul(1000))
}

/// Return `true` if `expires_at_ms` is missing (`0`), in the past, or within
/// `skew_ms` of `now_ms`. Pure: `now_ms` is passed in so callers stay testable.
pub fn is_expired(expires_at_ms: i64, now_ms: i64, skew_ms: i64) -> bool {
    if expires_at_ms <= 0 {
        return true;
    }
    expires_at_ms <= now_ms.saturating_add(skew_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    #[test]
    fn decodes_jwt_exp() {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let body = URL_SAFE_NO_PAD.encode(br#"{"exp":1700000000}"#);
        let token = format!("{header}.{body}.sig");
        assert_eq!(decode_jwt_exp_ms(&token), Some(1_700_000_000_000));
    }

    #[test]
    fn is_expired_handles_missing_and_past() {
        let now_ms = 1_700_000_000_000;
        assert!(is_expired(0, now_ms, 60_000));
        assert!(is_expired(1, now_ms, 60_000));
        let future = now_ms + 10 * 60_000;
        assert!(!is_expired(future, now_ms, 60_000));
        // Within the skew window counts as expired.
        assert!(is_expired(now_ms + 30_000, now_ms, 60_000));
    }
}
