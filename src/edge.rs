//! Edge inbound entry: bridges a WinterCG `fetch` event into the axum router.
//!
// TODO: drives a MINIMAL placeholder router. Wiring the full http::router(AppState) on wasm
//       is BLOCKED on edge cache/persistence (no wasm CacheBackend/PersistenceBackend impls yet).
//       This proves the inbound axum-on-wasm seam + Request/Response conversion only.

use axum::{Router, routing::get};
use bytes::Bytes;
use http_body_util::BodyExt;
use js_sys::Uint8Array;
use tower::ServiceExt;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Response, ResponseInit};

fn js_err(e: impl std::fmt::Debug) -> JsValue {
    JsValue::from_str(&format!("{e:?}"))
}

/// WinterCG fetch entry-point: receives an inbound Request, routes through the
/// minimal axum router, and returns a Response.
#[wasm_bindgen]
pub async fn fetch(req: web_sys::Request) -> Result<Response, JsValue> {
    let http_req = ws_request_to_http(req).await?;

    // Minimal router — proves the axum-on-wasm seam compiles and routes.
    let app: Router = Router::new().route("/healthz", get(|| async { "ok" }));
    let resp = app.oneshot(http_req).await.map_err(|e| js_err(e))?;

    http_response_to_ws(resp).await
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
