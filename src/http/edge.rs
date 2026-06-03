//! Edge inbound entry: bridges a WinterCG `fetch` event into the axum router.
//!
//! `init` builds the shared [`AppState`] from JS-host-passed credentials and
//! stashes it in a process-global `OnceLock`; `fetch` then routes every inbound
//! request through the SAME [`crate::http::server::router`] that native uses —
//! proving the inbound seam is shared across targets.
//!
//! `init` MUST be called exactly once before the first `fetch`. If `fetch`
//! runs before `init`, it returns a 503 with a clear message (an `AppState`
//! cannot be synthesised inside wasm without host-supplied credentials).

use std::sync::{Arc, OnceLock};

use bytes::Bytes;
use http_body_util::BodyExt;
use js_sys::Uint8Array;
use tower::ServiceExt;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Response, ResponseInit};

use crate::app::AppState;
use crate::config::{CacheConfig, PersistenceConfig, RuntimeConfig};
use crate::store::cache::{CacheBackend, LibsqlCache, UpstashCache};
use crate::store::persistence::{LibsqlPersistence, PersistenceBackend};

/// Process-global app state, populated once by [`init`].
static STATE: OnceLock<AppState> = OnceLock::new();

fn js_err(e: impl std::fmt::Debug) -> JsValue {
    JsValue::from_str(&format!("{e:?}"))
}

/// Initialise the edge runtime from host-supplied credentials.
///
/// Persistence is always libSQL/Turso (`turso_url` + `turso_token`). The cache
/// is Upstash Redis when both `upstash_url` and `upstash_token` are non-empty,
/// otherwise it falls back to the libSQL kv table.
///
/// Must be called once before [`fetch`]. A second call is a no-op (the first
/// `AppState` wins).
#[wasm_bindgen]
pub async fn init(
    turso_url: String,
    turso_token: String,
    upstash_url: Option<String>,
    upstash_token: Option<String>,
) -> Result<(), JsValue> {
    if STATE.get().is_some() {
        return Ok(());
    }

    let persistence: Arc<dyn PersistenceBackend> = Arc::new(LibsqlPersistence::connect(
        turso_url.clone(),
        turso_token.clone(),
    ));

    let (cache, cache_cfg): (Arc<dyn CacheBackend>, CacheConfig) =
        match (upstash_url, upstash_token) {
            (Some(u), Some(t)) if !u.is_empty() && !t.is_empty() => (
                Arc::new(UpstashCache::new(u.clone(), t)),
                CacheConfig::Redis { url: u },
            ),
            _ => {
                let c = LibsqlCache::connect(turso_url.clone(), turso_token.clone())
                    .await
                    .map_err(js_err)?;
                (Arc::new(c), CacheConfig::Memory)
            }
        };

    let config = Arc::new(RuntimeConfig {
        host: "0.0.0.0".to_string(),
        port: 0,
        cache: cache_cfg,
        persistence: PersistenceConfig::Db { dsn: turso_url },
        instance_id: 0,
    });

    let _ = STATE.set(AppState::new(config, cache, persistence));
    Ok(())
}

/// WinterCG fetch entry-point: receives an inbound Request, routes it through
/// the real [`crate::http::server::router`], and returns a Response.
///
/// Returns 503 if [`init`] has not yet been called.
#[wasm_bindgen]
pub async fn fetch(req: web_sys::Request) -> Result<Response, JsValue> {
    let Some(state) = STATE.get() else {
        return service_unavailable("gproxy edge not initialised: call init() first");
    };

    let http_req = ws_request_to_http(req).await?;
    let app = crate::http::server::router(state.clone());
    let resp = app.oneshot(http_req).await.map_err(js_err)?;
    http_response_to_ws(resp).await
}

/// Build a plain-text 503 Response.
fn service_unavailable(msg: &str) -> Result<Response, JsValue> {
    let init = ResponseInit::new();
    init.set_status(503);
    let mut body = msg.as_bytes().to_vec();
    Response::new_with_opt_u8_array_and_init(Some(&mut body), &init).map_err(js_err)
}

/// Convert `web_sys::Request` → `http::Request<axum::body::Body>`.
async fn ws_request_to_http(
    req: web_sys::Request,
) -> Result<http::Request<axum::body::Body>, JsValue> {
    let method = http::Method::from_bytes(req.method().as_bytes()).map_err(js_err)?;
    let uri: http::Uri = req.url().parse().map_err(js_err)?;

    // Read body via array_buffer.
    let body_bytes: Bytes = {
        let buf_promise = req.array_buffer().map_err(js_err)?;
        let buf_val = JsFuture::from(buf_promise).await.map_err(js_err)?;
        Uint8Array::new(&buf_val).to_vec().into()
    };

    // Copy headers.
    let mut builder = http::Request::builder().method(method).uri(uri);
    let ws_headers = req.headers();
    let header_iter = js_sys::try_iter(&ws_headers).map_err(js_err)?;
    if let Some(iter) = header_iter {
        for entry in iter {
            let entry = entry.map_err(js_err)?;
            let arr: js_sys::Array = entry.unchecked_into();
            let name = arr.get(0).as_string().unwrap_or_default();
            let val = arr.get(1).as_string().unwrap_or_default();
            // Skip entries with empty or unparseable names — a bad header must not
            // poison the whole builder (builder.header("", …) marks it as errored).
            if name.is_empty() {
                continue;
            }
            if let Ok(hn) = http::header::HeaderName::try_from(name.as_str()) {
                builder = builder.header(hn, val.as_str());
            }
        }
    }

    builder
        .body(axum::body::Body::from(body_bytes))
        .map_err(js_err)
}

/// Convert `http::Response<axum::body::Body>` → `web_sys::Response`.
async fn http_response_to_ws(resp: http::Response<axum::body::Body>) -> Result<Response, JsValue> {
    let (parts, body) = resp.into_parts();

    // Collect body bytes.
    let bytes: Bytes = body.collect().await.map_err(js_err)?.to_bytes();

    // Build ResponseInit with status + headers.
    let init = ResponseInit::new();
    init.set_status(parts.status.as_u16());

    let js_headers = Headers::new().map_err(js_err)?;
    for (name, value) in &parts.headers {
        if let Ok(v) = value.to_str() {
            js_headers.append(name.as_str(), v).map_err(js_err)?;
        }
    }
    init.set_headers_headers(&js_headers);

    let mut body_vec = bytes.to_vec();
    Response::new_with_opt_u8_array_and_init(Some(&mut body_vec), &init).map_err(js_err)
}
