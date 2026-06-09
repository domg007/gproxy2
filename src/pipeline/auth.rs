//! Inbound API-key authentication against the control-plane snapshot.

use std::sync::Arc;

use http::HeaderMap;
use http::header::AUTHORIZATION;

use crate::app::snapshot::{ControlPlaneSnapshot, KeyIdentity};
use crate::pipeline::error::PipelineError;

/// Extract the bearer token from inbound headers. Order:
/// 1. `Authorization: Bearer <tok>` (the `Bearer ` prefix stripped, ASCII
///    case-insensitive);
/// 2. else `x-api-key: <tok>` (Claude-style). Returns the BARE token.
pub fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        let v = v.trim();
        if let Some(rest) = v
            .strip_prefix("Bearer ")
            .or_else(|| v.strip_prefix("bearer "))
        {
            let rest = rest.trim();
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
            // empty Bearer value → fall through to x-api-key
        }
    }
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Digest used to index `keys_by_digest`. M1: lowercase hex of `blake3(token)`.
/// SINGLE SOURCE OF TRUTH — both seeding and [`authenticate`] call this on the
/// identical bare token. (Salt/pepper deferred to M6.)
pub fn key_digest(bare_token: &str) -> String {
    blake3::hash(bare_token.as_bytes()).to_hex().to_string()
}

/// Resolve an inbound API key → digest → snapshot identity. No DB hit. 401
/// short-circuits HERE, before any upstream candidate is built.
pub fn authenticate(
    cp: &ControlPlaneSnapshot,
    headers: &HeaderMap,
) -> Result<Arc<KeyIdentity>, PipelineError> {
    let token = extract_bearer(headers).ok_or(PipelineError::Unauthorized)?;
    let digest = key_digest(&token);
    cp.keys_by_digest
        .get(&digest)
        .cloned()
        .ok_or(PipelineError::Unauthorized)
}
