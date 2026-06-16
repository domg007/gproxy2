//! Kiro model-list (整形) — Kiro has no OpenAI-compatible `/v1/models`; the
//! model catalogue lives behind the bespoke Smithy REST endpoint
//! `GET {base}/ListAvailableModels?origin=…&maxResults=50&profileArn=…`.
//!
//! Two halves, both ported faithfully from v1 `channels/kiro.rs`:
//!   * [`request`] builds that GET (called from [`super::KiroChannel::prepare`]
//!     when the inbound request is the family model-list, i.e. a `GET /…/models`)
//!     with the same Bearer + AWS SDK fingerprint headers the IDE sends — the
//!     content path (`POST /generateAssistantResponse`) is left untouched.
//!   * [`to_openai`] reprojects the Kiro list response into the OpenAI canonical
//!     model-list wire shape so `parse_models` can read `data[].id`.
//!
//! Best-effort: the ORIGINAL bytes are returned on parse failure or when the
//! body is not in the expected Kiro shape.

use bytes::Bytes;
use http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderName, HeaderValue, USER_AGENT};
use http::{HeaderMap, Method, Request};
use serde_json::{Value, json};

use super::{DEFAULT_BASE_URL, ORIGIN, USER_AGENT_VALUE, auth};
use crate::channel::ChannelError;
use crate::channel::http_util::{build_request, join_url};

/// True when this inbound request is the family model-list request: the admin
/// model-pull sends `GET /v1/models` (OpenAi family). Detect it from method +
/// path so the content path (`POST /generateAssistantResponse`) is untouched.
pub(super) fn is_model_list(method: &Method, path: &str) -> bool {
    method == Method::GET && path.trim_end_matches('/').ends_with("/models")
}

/// Build the bespoke `GET {base}/ListAvailableModels?origin=…&maxResults=50&
/// profileArn=…` request with the Kiro Bearer + AWS SDK fingerprint headers.
/// Mirrors v1 `build_kiro_model_list_request`.
pub(super) fn request(secret: &Value, settings: &Value) -> Result<Request<Bytes>, ChannelError> {
    let access_token = auth::access_token(secret)?;
    let base = settings
        .get("base_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_BASE_URL);

    let mut path = format!("/ListAvailableModels?origin={ORIGIN}&maxResults=50");
    if let Some(arn) = auth::profile_arn(secret, settings) {
        path.push_str("&profileArn=");
        path.push_str(&pct(arn));
    }

    let uri = join_url(base, &path, None)?;
    let mut req = build_request(Method::GET, uri, HeaderMap::new(), Bytes::new())?;
    let bearer = HeaderValue::from_str(&format!("Bearer {access_token}"))
        .map_err(|e| ChannelError::InvalidCredential(format!("bad access_token: {e}")))?;
    let invocation = HeaderValue::from_str(&crate::util::rand::uuid_v4())
        .map_err(|e| ChannelError::Build(format!("bad invocation id: {e}")))?;
    let h = req.headers_mut();
    h.insert(AUTHORIZATION, bearer);
    h.insert(ACCEPT, HeaderValue::from_static("application/json"));
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    h.insert(
        HeaderName::from_static("x-amzn-codewhisperer-optout"),
        HeaderValue::from_static("true"),
    );
    h.insert(HeaderName::from_static("amz-sdk-invocation-id"), invocation);
    h.insert(
        HeaderName::from_static("amz-sdk-request"),
        HeaderValue::from_static("attempt=1; max=3"),
    );
    Ok(req)
}

/// Reproject the Kiro `ListAvailableModels` body into the OpenAI canonical list
/// envelope `{"object":"list","data":[{id, object:"model", created, owned_by:
/// "amazon"}]}`. Mirrors v1 `kiro_model_list_to_openai_model_list`. Returns the
/// input unchanged on parse failure or when there is no `models`/`data` array.
pub(super) fn to_openai(body: Bytes) -> Bytes {
    let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let Some(models) = payload
        .get("models")
        .or_else(|| payload.get("data"))
        .and_then(Value::as_array)
    else {
        return body;
    };

    let created = crate::util::time::unix_now();
    let data: Vec<Value> = models
        .iter()
        .filter_map(model_id)
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "created": created,
                "owned_by": "amazon",
            })
        })
        .collect();

    match serde_json::to_vec(&json!({ "object": "list", "data": data })) {
        Ok(out) => Bytes::from(out),
        Err(_) => body,
    }
}

/// Pull a model id from a Kiro list entry. Entries are objects keyed by
/// `modelId`/`model_id`/`id`/`name`; a bare string is taken verbatim (v1 parity).
fn model_id(value: &Value) -> Option<String> {
    match value {
        Value::String(id) => Some(id.clone()),
        Value::Object(_) => value
            .get("modelId")
            .or_else(|| value.get("model_id"))
            .or_else(|| value.get("id"))
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

/// Percent-encode a query value, leaving the RFC 3986 unreserved set verbatim
/// (`profileArn` carries `:` and `/`). Matches `usage::pct`.
fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(
                char::from_digit((b >> 4) as u32, 16)
                    .unwrap()
                    .to_ascii_uppercase(),
            );
            out.push(
                char::from_digit((b & 0xf) as u32, 16)
                    .unwrap()
                    .to_ascii_uppercase(),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builds_list_available_models_request() {
        let secret = json!({
            "access_token": "kiro-token",
            "profile_arn": "arn:aws:codewhisperer:us-east-1:1:profile/x",
        });
        let settings = json!({});
        let req = request(&secret, &settings).unwrap();

        assert_eq!(req.method(), Method::GET);
        // `base_url` defaults to the kiro REST host; the bespoke endpoint carries
        // the origin/maxResults/profileArn query (profileArn url-encoded).
        assert_eq!(
            req.uri().to_string(),
            format!(
                "{DEFAULT_BASE_URL}/ListAvailableModels?origin=AI_EDITOR&maxResults=50\
                 &profileArn=arn%3Aaws%3Acodewhisperer%3Aus-east-1%3A1%3Aprofile%2Fx"
            )
        );
        assert!(req.body().is_empty());
        assert_eq!(
            req.headers().get("authorization").unwrap(),
            "Bearer kiro-token"
        );
        assert_eq!(
            req.headers().get("x-amzn-codewhisperer-optout").unwrap(),
            "true"
        );
        assert!(req.headers().get("amz-sdk-invocation-id").is_some());
    }

    #[test]
    fn omits_profile_arn_when_absent() {
        let req = request(&json!({ "access_token": "t" }), &json!({})).unwrap();
        assert_eq!(
            req.uri().to_string(),
            format!("{DEFAULT_BASE_URL}/ListAvailableModels?origin=AI_EDITOR&maxResults=50")
        );
    }

    #[test]
    fn reshapes_kiro_list_to_openai() {
        // v1-shaped response: `models` entries keyed by modelId/name; bare ids and
        // entries lacking any id key are handled (the latter dropped).
        let body = Bytes::from_static(
            br#"{"models":[{"modelId":"claude-sonnet-4.5"},{"name":"amazonq"},{"foo":"bar"}]}"#,
        );
        let out = to_openai(body);
        let v: Value = serde_json::from_slice(&out).unwrap();

        assert_eq!(v["object"], "list");
        let data = v["data"].as_array().unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0]["id"], "claude-sonnet-4.5");
        assert_eq!(data[0]["object"], "model");
        assert_eq!(data[0]["owned_by"], "amazon");
        assert!(data[0]["created"].is_number());
        assert_eq!(data[1]["id"], "amazonq");
    }

    #[test]
    fn reshape_passthrough_on_bad_input() {
        // No `models`/`data` array → returned verbatim.
        let body = Bytes::from_static(br#"{"unexpected":true}"#);
        assert_eq!(to_openai(body.clone()), body);

        // Non-JSON → returned verbatim.
        let bad = Bytes::from_static(b"not json");
        assert_eq!(to_openai(bad.clone()), bad);
    }
}
