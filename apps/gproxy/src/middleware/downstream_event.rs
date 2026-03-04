use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::header::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use futures_util::StreamExt;
use gproxy_core::{AppState, INTERNAL_DOWNSTREAM_TRACE_ID_HEADER};
use gproxy_storage::{DownstreamRequestWrite, StorageWriteEvent};

const X_API_KEY: &str = "x-api-key";
const BODY_CAPTURE_LIMIT_BYTES: usize = 50 * 1024 * 1024;

#[derive(Clone)]
struct DownstreamRequestEventBase {
    trace_id: i64,
    at_unix_ms: i64,
    internal: bool,
    user_id: Option<i64>,
    user_key_id: Option<i64>,
    request_method: String,
    request_headers_json: String,
    request_path: String,
    request_query: Option<String>,
    request_body: Option<Vec<u8>>,
    response_status: Option<i32>,
    response_headers_json: String,
}

impl DownstreamRequestEventBase {
    fn build_event(self, response_body: Option<Vec<u8>>) -> DownstreamRequestWrite {
        DownstreamRequestWrite {
            trace_id: self.trace_id,
            at_unix_ms: self.at_unix_ms,
            internal: self.internal,
            user_id: self.user_id,
            user_key_id: self.user_key_id,
            request_method: self.request_method,
            request_headers_json: self.request_headers_json,
            request_path: self.request_path,
            request_query: self.request_query,
            request_body: self.request_body,
            response_status: self.response_status,
            response_headers_json: self.response_headers_json,
            response_body,
        }
    }
}

#[derive(Clone)]
struct DownstreamStreamRecordContext {
    state: Arc<AppState>,
    trace_id: i64,
    path: String,
    event_base: DownstreamRequestEventBase,
    mask_sensitive_info: bool,
}

#[derive(Default)]
struct DownstreamStreamRecordState {
    captured: Vec<u8>,
    capture_truncated: bool,
    flushed: bool,
}

struct DownstreamStreamRecordGuard {
    context: DownstreamStreamRecordContext,
    state: Arc<Mutex<DownstreamStreamRecordState>>,
}

impl DownstreamStreamRecordGuard {
    fn new(context: DownstreamStreamRecordContext) -> Self {
        Self {
            context,
            state: Arc::new(Mutex::new(DownstreamStreamRecordState::default())),
        }
    }

    fn push_chunk(&self, chunk: &[u8]) {
        if self.context.mask_sensitive_info {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.capture_truncated {
            return;
        }
        let remaining = BODY_CAPTURE_LIMIT_BYTES.saturating_sub(state.captured.len());
        if remaining > 0 {
            let take = chunk.len().min(remaining);
            state.captured.extend_from_slice(&chunk[..take]);
        }
        if state.captured.len() >= BODY_CAPTURE_LIMIT_BYTES {
            state.capture_truncated = true;
        }
    }

    fn take_event(&self) -> Option<DownstreamRequestWrite> {
        let Ok(mut state) = self.state.lock() else {
            return None;
        };
        if state.flushed {
            return None;
        }
        state.flushed = true;
        let response_body = if self.context.mask_sensitive_info {
            None
        } else {
            (!state.captured.is_empty()).then(|| std::mem::take(&mut state.captured))
        };
        Some(self.context.event_base.clone().build_event(response_body))
    }

    async fn flush_now(&self) {
        let Some(event) = self.take_event() else {
            return;
        };
        if let Err(err) = self
            .context
            .state
            .enqueue_storage_write(StorageWriteEvent::UpsertDownstreamRequest(event))
            .await
        {
            tracing::warn!(
                trace_id=%self.context.trace_id,
                path=%self.context.path,
                error=%err,
                "downstream event enqueue failed"
            );
        }
    }
}

impl Drop for DownstreamStreamRecordGuard {
    fn drop(&mut self) {
        let Some(event) = self.take_event() else {
            return;
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let context = self.context.clone();
        handle.spawn(async move {
            if let Err(err) = context
                .state
                .enqueue_storage_write(StorageWriteEvent::UpsertDownstreamRequest(event))
                .await
            {
                tracing::warn!(
                    trace_id=%context.trace_id,
                    path=%context.path,
                    error=%err,
                    "downstream event enqueue failed"
                );
            }
        });
    }
}

fn wrap_stream_with_downstream_record(
    response_body_stream: Body,
    context: DownstreamStreamRecordContext,
) -> Body {
    let guard = DownstreamStreamRecordGuard::new(context);
    let mut data_stream = response_body_stream.into_data_stream();
    let passthrough = async_stream::stream! {
        while let Some(item) = data_stream.next().await {
            match item {
                Ok(chunk) => {
                    guard.push_chunk(chunk.as_ref());
                    yield Ok::<Bytes, axum::Error>(chunk);
                }
                Err(err) => {
                    guard.flush_now().await;
                    yield Err::<Bytes, axum::Error>(err);
                    return;
                }
            }
        }
        guard.flush_now().await;
    };
    Body::from_stream(passthrough)
}

pub async fn middleware(
    State(state): State<Arc<AppState>>,
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
    let mut request = Request::from_parts(request_parts, Body::from(request_body_bytes));
    if let Ok(value) = HeaderValue::from_str(trace_id.to_string().as_str()) {
        request.headers_mut().insert(
            HeaderName::from_static(INTERNAL_DOWNSTREAM_TRACE_ID_HEADER),
            value,
        );
    }

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
    let event_base = DownstreamRequestEventBase {
        trace_id,
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
    };
    if response_is_stream {
        let stream_record_context = DownstreamStreamRecordContext {
            state: state.clone(),
            trace_id,
            path: path.clone(),
            event_base,
            mask_sensitive_info,
        };
        let response = Response::from_parts(
            response_parts,
            wrap_stream_with_downstream_record(response_body_stream, stream_record_context),
        );
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
        return response;
    }
    let (response_body, response) = {
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
    let event = event_base.build_event(response_body);

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
