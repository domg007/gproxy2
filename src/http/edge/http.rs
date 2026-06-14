//! Edge HTTP parse/build helpers — path segmenting, query/body decoding and
//! `web_sys::Response` construction. Compiled only under the wasm edge build
//! (this module lives inside `crate::http::edge`, which is already cfg-gated).
//!
//! All public items in this module are consumed by `edge/admin.rs` (Task 4).
//! The `dead_code` lint is suppressed here because the admin dispatcher that
//! calls these helpers is added in the next task.
#![allow(dead_code)]

use bytes::Bytes;
// Use `::http` to unambiguously reference the external crate (this module is
// named `http`, which would otherwise shadow the crate name).
use ::http::request::Parts;
use serde::{Serialize, de::DeserializeOwned};
use wasm_bindgen::JsValue;
use web_sys::{Headers, Response};

use crate::api::error::ApiError;

// ── Path helpers ─────────────────────────────────────────────────────────────

/// Split a URI path into non-empty segments.
///
/// `/a/b/c` → `["a", "b", "c"]`; trailing/double slashes are ignored.
pub fn segments(parts: &Parts) -> Vec<&str> {
    parts
        .uri
        .path()
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse a path segment as `i64`, mapping parse errors to [`ApiError::BadRequest`].
pub fn parse_i64(seg: &str) -> Result<i64, ApiError> {
    seg.parse::<i64>()
        .map_err(|_| ApiError::BadRequest(format!("invalid id: {seg}")))
}

// ── Body/query decoding ───────────────────────────────────────────────────────

/// Deserialize a JSON request body, mapping errors to [`ApiError::BadRequest`].
pub fn json_body<T: DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body)
        .map_err(|e| ApiError::BadRequest(format!("invalid JSON body: {e}")))
}

/// Deserialize URL-encoded query parameters from `parts.uri.query()`.
///
/// An absent query string is treated as an empty string — if `T` derives
/// `Default` and all fields are optional, this succeeds with defaults.
pub fn query<T: DeserializeOwned>(parts: &Parts) -> Result<T, ApiError> {
    serde_urlencoded::from_str(parts.uri.query().unwrap_or(""))
        .map_err(|e| ApiError::BadRequest(format!("invalid query: {e}")))
}

// ── Response builders ─────────────────────────────────────────────────────────

/// Serialize `value` as JSON and return a `web_sys::Response` with the given
/// status and `content-type: application/json`.
pub fn json_response<T: Serialize>(status: u16, value: &T) -> Result<Response, JsValue> {
    let bytes = serde_json::to_vec(value)
        .map_err(|e| JsValue::from_str(&format!("json serialization failed: {e}")))?;
    let headers = Headers::new().map_err(js_err)?;
    headers
        .append("content-type", "application/json")
        .map_err(js_err)?;
    super::js_response(status, &headers, &bytes)
}

/// Convert an [`ApiError`] into a `web_sys::Response` using [`ApiError::to_parts`].
pub fn api_err_response(e: &ApiError) -> Result<Response, JsValue> {
    let (status, bytes) = e.to_parts();
    let headers = Headers::new().map_err(js_err)?;
    headers
        .append("content-type", "application/json")
        .map_err(js_err)?;
    super::js_response(status.as_u16(), &headers, &bytes)
}

/// Build a response with a `Set-Cookie` header.
///
/// Used by the login/logout flows (B6.3) to mint or clear the session cookie.
/// The `body` is assumed to be JSON; `content-type: application/json` is added.
pub fn with_set_cookie(status: u16, set_cookie: &str, body: &[u8]) -> Result<Response, JsValue> {
    let headers = Headers::new().map_err(js_err)?;
    headers
        .append("content-type", "application/json")
        .map_err(js_err)?;
    headers.append("set-cookie", set_cookie).map_err(js_err)?;
    super::js_response(status, &headers, body)
}

// ── Internal ──────────────────────────────────────────────────────────────────

fn js_err(e: impl std::fmt::Debug) -> JsValue {
    JsValue::from_str(&format!("{e:?}"))
}
