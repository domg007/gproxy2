use axum::http::HeaderMap;

use gproxy_server::AppState;
use gproxy_server::principal::MemoryUserKey;

use crate::error::HttpError;

/// Extract API key from request headers.
/// Checks: Authorization: Bearer <key>, x-api-key, x-goog-api-key.
pub fn extract_api_key(headers: &HeaderMap) -> Result<String, HttpError> {
    // Authorization: Bearer <key>
    if let Some(value) = headers.get("authorization")
        && let Ok(s) = value.to_str()
        && let Some(token) = s
            .strip_prefix("Bearer ")
            .or_else(|| s.strip_prefix("bearer "))
    {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    // x-api-key
    if let Some(value) = headers.get("x-api-key")
        && let Ok(s) = value.to_str()
    {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    // x-goog-api-key
    if let Some(value) = headers.get("x-goog-api-key")
        && let Ok(s) = value.to_str()
    {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    Err(HttpError::unauthorized("missing API key"))
}

/// Authenticate a user API key and return the key record.
pub fn authenticate_user(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<MemoryUserKey, HttpError> {
    let api_key = extract_api_key(headers)?;
    state
        .authenticate_api_key(&api_key)
        .ok_or_else(|| HttpError::unauthorized("invalid or disabled API key"))
}

/// Authenticate as admin (check against global admin_key).
pub fn authorize_admin(headers: &HeaderMap, state: &AppState) -> Result<(), HttpError> {
    let api_key = extract_api_key(headers)?;
    let config = state.config();
    if api_key == config.admin_key {
        Ok(())
    } else {
        Err(HttpError::forbidden("admin access required"))
    }
}
