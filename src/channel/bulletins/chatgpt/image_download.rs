//! Conversation polling + image-byte download for the chatgpt image flow.
//!
//! Image generation on chatgpt.com is asynchronous: the `/f/conversation`
//! response only acknowledges the job ("Processing image" tool message); the
//! real `file-service://` / `sediment://` pointers appear later in
//! `GET /backend-api/conversation/{cid}`. [`poll_conversation_for_images`]
//! polls that endpoint until they show up or a deadline passes, then
//! [`download_image_b64`] resolves each pointer to base64 bytes.
//!
//! Ported from v1 `channels/chatgpt/image.rs`, adapted from `wreq::Client` to
//! the v2 [`UpstreamClient`] and `crate::util::time` clocks.

use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::Bytes;
use serde_json::Value;

use super::headers::standard_headers;
use super::images::{ImagePointer, collect_pointers_from_parts};
use crate::http::client::UpstreamClient;

/// Read the access token + device id from the secret (both required to
/// authenticate the file endpoints).
fn auth_pair(secret: &Value) -> Result<(String, String), String> {
    let token = secret
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing access_token".to_string())?
        .to_string();
    let device = secret
        .get("device_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok((token, device))
}

/// Build a GET request carrying the standard chatgpt-web headers plus the
/// `oai-device-id`. Used for the authenticated file-service endpoints.
fn authed_get(url: &str, token: &str, device: &str) -> Result<http::Request<Bytes>, String> {
    let mut req = http::Request::get(url)
        .body(Bytes::new())
        .map_err(|e| format!("build request: {e}"))?;
    *req.headers_mut() = standard_headers(token);
    if !device.is_empty()
        && let Ok(v) = http::HeaderValue::from_str(device)
    {
        req.headers_mut()
            .insert(http::HeaderName::from_static("oai-device-id"), v);
    }
    Ok(req)
}

/// Poll `GET /backend-api/conversation/{cid}` every 3s until image pointers
/// (tool / `image_gen` / `multimodal_text`) appear or `deadline_secs` passes.
pub(super) async fn poll_conversation_for_images(
    client: &Arc<dyn UpstreamClient>,
    secret: &Value,
    base: &str,
    conversation_id: &str,
    deadline_secs: u64,
) -> Result<Vec<ImagePointer>, String> {
    let (token, device) = auth_pair(secret)?;
    let url = format!("{base}/backend-api/conversation/{conversation_id}");
    let deadline_ms = crate::util::time::unix_now_ms() + deadline_secs * 1000;

    loop {
        let req = authed_get(&url, &token, &device)?;
        let resp = client
            .send(req)
            .await
            .map_err(|e| format!("poll conv: {e}"))?;
        if !resp.status().is_success() {
            if crate::util::time::unix_now_ms() >= deadline_ms {
                return Err(format!("poll conv timed out with status {}", resp.status()));
            }
            crate::util::time::sleep_ms(3000).await;
            continue;
        }
        let json: Value = match serde_json::from_slice(resp.body()) {
            Ok(v) => v,
            Err(_) => {
                if crate::util::time::unix_now_ms() >= deadline_ms {
                    return Err("poll conv: bad body".into());
                }
                crate::util::time::sleep_ms(3000).await;
                continue;
            }
        };
        let ids = scan_mapping_for_pointers(&json);
        if !ids.is_empty() {
            return Ok(ids
                .into_iter()
                .map(|id| ImagePointer {
                    id,
                    conversation_id: conversation_id.to_string(),
                })
                .collect());
        }
        if crate::util::time::unix_now_ms() >= deadline_ms {
            return Err("poll conv: no image pointers before deadline".into());
        }
        crate::util::time::sleep_ms(3000).await;
    }
}

/// Walk the conversation `mapping` for tool messages of an `image_gen` async
/// task whose content is `multimodal_text`, collecting asset pointers.
fn scan_mapping_for_pointers(json: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    let Some(mapping) = json.get("mapping").and_then(|m| m.as_object()) else {
        return ids;
    };
    for node in mapping.values() {
        let Some(msg) = node.get("message") else {
            continue;
        };
        let role = msg
            .get("author")
            .and_then(|a| a.get("role"))
            .and_then(Value::as_str);
        if role != Some("tool") {
            continue;
        }
        let async_kind = msg
            .get("metadata")
            .and_then(|m| m.get("async_task_type"))
            .and_then(Value::as_str);
        if async_kind != Some("image_gen") {
            continue;
        }
        let content_type = msg
            .get("content")
            .and_then(|c| c.get("content_type"))
            .and_then(Value::as_str);
        if content_type != Some("multimodal_text") {
            continue;
        }
        if let Some(parts) = msg
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
        {
            collect_pointers_from_parts(parts, &mut ids);
        }
    }
    ids
}

/// Resolve one image pointer to standard-base64 bytes via the 2-step download:
/// step1 (authenticated) yields a presigned `download_url`; step2 fetches that
/// url. Both steps are authenticated (Bearer + device): the resolved url is now
/// a SAME-HOST `chatgpt.com/backend-api/estuary/content` endpoint that 403s
/// ("File stream access denied") if fetched without the API auth.
pub(super) async fn download_image_b64(
    client: &Arc<dyn UpstreamClient>,
    secret: &Value,
    base: &str,
    ptr: &ImagePointer,
) -> Result<String, String> {
    let (token, device) = auth_pair(secret)?;
    let endpoint_id = ptr.id.strip_prefix("sed:").unwrap_or(&ptr.id);
    // Route by ID FORMAT, not the pointer's URL scheme. The backend now wraps
    // image-gen `file_…` ids in a `sediment://` scheme (April used
    // `file-service://`), but they are still downloaded via the file-service
    // endpoint — the conversation `attachment` endpoint returns a `download_url`
    // that 403s ("File stream access denied"). Only genuine non-`file_` sediment
    // ids use the attachment endpoint.
    let is_file_service = endpoint_id.starts_with("file_") || endpoint_id.starts_with("file-");
    let step1_url = if is_file_service {
        format!(
            "{base}/backend-api/files/download/{}?conversation_id={}&inline=false",
            endpoint_id, ptr.conversation_id
        )
    } else {
        format!(
            "{base}/backend-api/conversation/{}/attachment/{}/download",
            ptr.conversation_id, endpoint_id
        )
    };

    let step1 = client
        .send(authed_get(&step1_url, &token, &device)?)
        .await
        .map_err(|e| format!("download step1: {e}"))?;
    if !step1.status().is_success() {
        return Err(format!(
            "download step1 {}: {}",
            step1.status(),
            snippet(step1.body())
        ));
    }
    let parsed: Value =
        serde_json::from_slice(step1.body()).map_err(|e| format!("download step1 decode: {e}"))?;
    let download_url = parsed
        .get("download_url")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing download_url in step1: {}", snippet(step1.body())))?;

    // Step 2: fetch the resolved `download_url`. The backend now returns a
    // SAME-HOST url (`chatgpt.com/backend-api/estuary/content?…`) rather than a
    // separate no-auth storage host, so it needs the SAME authenticated header
    // set as step1 (Bearer + device) — fetching it bare yields
    // 403 "File stream access denied". We add the image `accept` + the
    // conversation `referer` the browser sends.
    let mut step2_req = authed_get(download_url, &token, &device)?;
    let h = step2_req.headers_mut();
    h.insert(
        http::header::ACCEPT,
        http::HeaderValue::from_static(
            "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
        ),
    );
    h.insert(
        http::header::REFERER,
        http::HeaderValue::from_str(&format!("{base}/c/{}", ptr.conversation_id))
            .unwrap_or_else(|_| http::HeaderValue::from_static("https://chatgpt.com/")),
    );
    let step2 = client
        .send(step2_req)
        .await
        .map_err(|e| format!("download step2: {e}"))?;
    if !step2.status().is_success() {
        return Err(format!(
            "download step2 {}: {}",
            step2.status(),
            snippet(step2.body())
        ));
    }
    Ok(STANDARD.encode(step2.body()))
}

/// First 200 chars of a (possibly non-UTF8) body, for error context.
fn snippet(body: &[u8]) -> String {
    String::from_utf8_lossy(body).chars().take(200).collect()
}
