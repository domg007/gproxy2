//! Inbound request → [`RequestCtx`] extraction (request-id, routing mode, path
//! normalization). Body reading + the body-size limit live in the gateway.

use bytes::Bytes;
use http::request::Parts;

use crate::pipeline::context::{RequestCtx, RoutingMode};
use crate::pipeline::error::PipelineError;

/// Max inbound body accepted by the gateway (matches the router's body limit).
pub const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Build a [`RequestCtx`] from request parts + the already-read body. For scoped
/// mode the leading `/{provider}` segment is stripped so `path` is `/v1/...` in
/// both modes.
pub fn build_ctx(parts: Parts, body: Bytes, scoped: bool) -> Result<RequestCtx, PipelineError> {
    let query = parts.uri.query().map(|q| q.to_string());
    let raw_path = parts.uri.path();

    let (mode, path) = if scoped {
        let trimmed = raw_path.trim_start_matches('/');
        let (provider, rest) = trimmed
            .split_once('/')
            .ok_or(PipelineError::UnsupportedPath)?;
        if provider.is_empty() || provider == "v1" {
            return Err(PipelineError::UnsupportedPath);
        }
        (
            RoutingMode::Scoped {
                provider: provider.to_string(),
            },
            format!("/{rest}"),
        )
    } else {
        (RoutingMode::Aggregated, raw_path.to_string())
    };

    Ok(RequestCtx {
        request_id: gen_request_id(),
        method: parts.method,
        path,
        query,
        headers: parts.headers,
        body,
        mode,
        identity: None,
        op: None,
        stream: false,
        route_name: None,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn gen_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(target_arch = "wasm32")]
fn gen_request_id() -> String {
    use core::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{:x}", js_sys::Date::now() as u64, n)
}
