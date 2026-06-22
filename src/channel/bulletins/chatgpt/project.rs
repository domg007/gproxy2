//! ChatGPT "project" (snorlax gizmo) resolution for the `project` session mode.
//!
//! Resolves a project NAME (default `gproxy`) to its `g-p-…` gizmo id —
//! find-or-create — and caches it process-globally per (account, name) so the
//! hot path is a single map lookup. [`cached`] is a synchronous read used by
//! `prepare()`; [`resolve`] is the async find-or-create used by the channel's
//! async send paths on first use. Endpoints mined live (June 2026):
//!   - find: `GET /backend-api/gizmos/snorlax/sidebar` → `items[].gizmo`
//!     (`.id` = `g-p-…`, `.display.name`).
//!   - create: `POST /backend-api/projects` `{name, instructions:""}` →
//!     `{resource:{gizmo:{id:"g-p-…"}}}`.
//!
//! On any error `resolve` returns `None` so the caller degrades to a normal
//! persistent chat for that turn (resolution retries next turn).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use bytes::Bytes;
use serde_json::{Value, json};

use crate::http::client::UpstreamClient;

fn cache() -> &'static Mutex<HashMap<String, String>> {
    static C: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Per-(account, name) cache key. `device_id` keys the account (stable per
/// credential); falls back to the access-token prefix so distinct accounts
/// never share a resolved project id.
fn cache_key(secret: &Value, base: &str, name: &str) -> String {
    let acct = secret
        .get("device_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            secret
                .get("access_token")
                .and_then(Value::as_str)
                .map(|t| &t[..t.len().min(16)])
        })
        .unwrap_or("");
    format!("{base}\u{1}{acct}\u{1}{name}")
}

/// Synchronous cache read — `Some(project_id)` once resolved, else `None`.
pub(super) fn cached(secret: &Value, base: &str, name: &str) -> Option<String> {
    cache()
        .lock()
        .ok()?
        .get(&cache_key(secret, base, name))
        .cloned()
}

/// Resolve `name` to a `g-p-…` project id (find, else create), caching it.
/// `None` on any failure — the caller degrades to a normal persistent chat.
pub(super) async fn resolve(
    client: &std::sync::Arc<dyn UpstreamClient>,
    secret: &Value,
    base: &str,
    name: &str,
) -> Option<String> {
    let key = cache_key(secret, base, name);
    if let Some(id) = cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Some(id);
    }
    let id = match find(client, secret, base, name).await {
        Some(id) => id,
        None => create(client, secret, base, name).await?,
    };
    if let Ok(mut c) = cache().lock() {
        c.insert(key, id.clone());
    }
    Some(id)
}

/// Find an existing project by `display.name` in the snorlax sidebar.
async fn find(
    client: &std::sync::Arc<dyn UpstreamClient>,
    secret: &Value,
    base: &str,
    name: &str,
) -> Option<String> {
    let url = format!("{base}/backend-api/gizmos/snorlax/sidebar");
    let mut req = http::Request::get(url).body(Bytes::new()).ok()?;
    super::auth::apply_request_headers(&mut req, secret).ok()?;
    let resp = client.send(req).await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = serde_json::from_slice(resp.body()).ok()?;
    v.get("items")?.as_array()?.iter().find_map(|it| {
        let g = it.get("gizmo")?;
        if g.pointer("/display/name").and_then(Value::as_str) == Some(name) {
            g.get("id").and_then(Value::as_str).map(str::to_string)
        } else {
            None
        }
    })
}

/// Create a new project named `name`.
async fn create(
    client: &std::sync::Arc<dyn UpstreamClient>,
    secret: &Value,
    base: &str,
    name: &str,
) -> Option<String> {
    let url = format!("{base}/backend-api/projects");
    let body = serde_json::to_vec(&json!({ "name": name, "instructions": "" })).ok()?;
    let mut req = http::Request::post(url).body(Bytes::from(body)).ok()?;
    super::auth::apply_request_headers(&mut req, secret).ok()?;
    req.headers_mut().insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    let resp = client.send(req).await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = serde_json::from_slice(resp.body()).ok()?;
    v.pointer("/resource/gizmo/id")
        .and_then(Value::as_str)
        .map(str::to_string)
}
