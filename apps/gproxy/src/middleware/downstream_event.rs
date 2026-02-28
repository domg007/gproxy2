use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::Request;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use gproxy_core::AppState;
use gproxy_storage::{DownstreamRequestWrite, StorageWriteEvent};

const X_API_KEY: &str = "x-api-key";
const BODY_CAPTURE_LIMIT_BYTES: usize = 32 * 1024 * 1024;

pub async fn middleware(
    State(state): State<std::sync::Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let trace_id = next_trace_id();
    let at_unix_ms = now_unix_ms();
    let mask_sensitive_info = state.config.load().global.mask_sensitive_info;

    let (request_parts, request_body_stream) = request.into_parts();
    let method = request_parts.method.to_string();
    let path = request_parts.uri.path().to_string();
    let query = request_parts.uri.query().map(ToOwned::to_owned);
    let request_headers_json = headers_to_json(&request_parts.headers);
    let request_body_bytes = axum::body::to_bytes(request_body_stream, BODY_CAPTURE_LIMIT_BYTES)
        .await
        .unwrap_or_default();
    let request_body = if mask_sensitive_info {
        None
    } else {
        (!request_body_bytes.is_empty()).then(|| request_body_bytes.to_vec())
    };
    let request = Request::from_parts(request_parts, Body::from(request_body_bytes));

    let authenticated = request
        .headers()
        .get(X_API_KEY)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| state.authenticate_api_key_in_memory(value));

    let user_id = authenticated.as_ref().map(|row| row.user_id);
    let user_key_id = authenticated.as_ref().map(|row| row.id);
    let internal = !path.starts_with("/v1");

    let response = next.run(request).await;
    let (response_parts, response_body_stream) = response.into_parts();
    let response_status = Some(i32::from(response_parts.status.as_u16()));
    let response_headers_json = headers_to_json(&response_parts.headers);
    let response_is_stream = response_parts
        .headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            let lowered = value.to_ascii_lowercase();
            lowered.contains("text/event-stream") || lowered.contains("application/x-ndjson")
        })
        .unwrap_or(false);
    let (response_body, response) = if response_is_stream {
        (
            None,
            Response::from_parts(response_parts, response_body_stream),
        )
    } else {
        let body_bytes = axum::body::to_bytes(response_body_stream, BODY_CAPTURE_LIMIT_BYTES)
            .await
            .unwrap_or_default();
        let body = if mask_sensitive_info {
            None
        } else {
            (!body_bytes.is_empty()).then(|| body_bytes.to_vec())
        };
        (
            body,
            Response::from_parts(response_parts, Body::from(body_bytes)),
        )
    };

    let event = DownstreamRequestWrite {
        at_unix_ms,
        internal,
        user_id,
        user_key_id,
        request_method: method.clone(),
        request_headers_json,
        request_path: path.clone(),
        request_query: query.clone(),
        request_body,
        response_status,
        response_headers_json,
        response_body,
    };

    if let Err(err) = state
        .enqueue_storage_write(StorageWriteEvent::UpsertDownstreamRequest(event))
        .await
    {
        tracing::warn!(trace_id=%trace_id, path=%path, error=%err, "downstream event enqueue failed");
    }

    tracing::info!(
        trace_id=%trace_id,
        internal,
        user_id=?user_id,
        user_key_id=?user_key_id,
        method=%method,
        path=%path,
        query=?query,
        status=?response_status,
        "downstream request"
    );

    response
}

fn now_unix_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

fn next_trace_id() -> i64 {
    static TRACE_BASE: OnceLock<i64> = OnceLock::new();
    static NEXT_TRACE_OFFSET: AtomicI64 = AtomicI64::new(0);
    let base = *TRACE_BASE.get_or_init(|| now_unix_ms().saturating_mul(1000));
    base.saturating_add(NEXT_TRACE_OFFSET.fetch_add(1, Ordering::Relaxed))
}

fn headers_to_json(headers: &HeaderMap) -> String {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, value) in headers {
        map.entry(name.as_str().to_string())
            .or_default()
            .push(value.to_str().unwrap_or_default().to_string());
    }
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
}
