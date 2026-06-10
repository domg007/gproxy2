//! Google Code Assist envelope (shared by `geminicli` + `antigravity`).
//!
//! Code Assist wraps the standard Gemini `generateContent` API:
//!   * **Request** — a gemini body is nested under `request`, alongside routing
//!     metadata: `{model, project, user_prompt_id, request:<body>}`. Code Assist
//!     additionally requires every `contents[].role` to be present, so a missing
//!     role is forced to `"user"`.
//!   * **Response** — the gemini payload is nested under `.response`; unwrap it.
//!   * **Stream** — each SSE frame's `data:` is a `{"response":{…}}` object;
//!     unwrap the inner payload per frame so the downstream M2 transform sees
//!     canonical gemini `alt=sse` frames.
//!
//! All helpers are total: a body that does not match the envelope shape is
//! returned unchanged (Code Assist is the only producer, but tolerance keeps a
//! mis-routed body from being corrupted).

use bytes::Bytes;
use serde_json::{Value, json};

use crate::channel::{ChannelError, ChannelStreamDecoder};
use crate::transform::common::sse::SseDecoder;

/// Wrap a gemini `generateContent` body in the Code Assist request envelope.
/// `user_prompt_id` is caller-generated (random hex). Forces a missing
/// `contents[].role` to `"user"` (Code Assist rejects role-less turns).
pub fn wrap_code_assist(
    gemini_body: &[u8],
    model: &str,
    project: &str,
    user_prompt_id: &str,
) -> Result<Vec<u8>, ChannelError> {
    let mut request: Value = serde_json::from_slice(gemini_body)
        .map_err(|e| ChannelError::Build(format!("gemini body is not JSON: {e}")))?;
    force_user_roles(&mut request);
    let envelope = json!({
        "model": model,
        "project": project,
        "user_prompt_id": user_prompt_id,
        "request": request,
    });
    serde_json::to_vec(&envelope)
        .map_err(|e| ChannelError::Build(format!("serialize code-assist envelope: {e}")))
}

/// Unwrap a Code Assist response: extract `.response`. On a parse failure or a
/// missing `.response` the original body is returned unchanged.
pub fn unwrap_code_assist(body: Bytes) -> Bytes {
    match unwrap_response_bytes(&body) {
        Some(inner) => Bytes::from(inner),
        None => body,
    }
}

/// Set `contents[].role = "user"` for any turn missing a role.
fn force_user_roles(request: &mut Value) {
    if let Some(contents) = request.get_mut("contents").and_then(Value::as_array_mut) {
        for turn in contents {
            if let Some(obj) = turn.as_object_mut()
                && !obj.contains_key("role")
            {
                obj.insert("role".to_string(), Value::String("user".to_string()));
            }
        }
    }
}

/// Parse `raw` and reserialize its `.response`; `None` if it does not parse or
/// has no `.response` field.
fn unwrap_response_bytes(raw: &[u8]) -> Option<Vec<u8>> {
    let v: Value = serde_json::from_slice(raw).ok()?;
    let inner = v.get("response")?;
    serde_json::to_vec(inner).ok()
}

/// Per-frame Code Assist stream unwrap (envelope SSE → canonical gemini SSE).
/// Decodes upstream SSE frames, extracts `.response` from each `data:` payload,
/// and re-emits `data: {inner}\n\n`. A frame whose data is not an envelope is
/// passed through verbatim (re-encoded as a `data:` frame).
#[derive(Default)]
pub struct CodeAssistStreamDecoder {
    decoder: SseDecoder,
}

impl CodeAssistStreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn emit(data: &str, out: &mut Vec<u8>) {
        let unwrapped = unwrap_response_bytes(data.as_bytes());
        let payload = match &unwrapped {
            Some(inner) => std::str::from_utf8(inner).unwrap_or(data),
            None => data,
        };
        out.extend_from_slice(b"data: ");
        out.extend_from_slice(payload.as_bytes());
        out.extend_from_slice(b"\n\n");
    }
}

impl ChannelStreamDecoder for CodeAssistStreamDecoder {
    fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for frame in self.decoder.push(chunk) {
            Self::emit(&frame.data, &mut out);
        }
        out
    }

    fn finish(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(frame) = self.decoder.finish() {
            Self::emit(&frame.data, &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_assist_roundtrip() {
        // wrap: envelope carries the four fields; role defaulted to "user".
        let body = br#"{"contents":[{"parts":[{"text":"hi"}]}]}"#;
        let wrapped = wrap_code_assist(body, "gemini-2.0", "proj-1", "pid-abc").unwrap();
        let v: Value = serde_json::from_slice(&wrapped).unwrap();
        assert_eq!(v["model"], "gemini-2.0");
        assert_eq!(v["project"], "proj-1");
        assert_eq!(v["user_prompt_id"], "pid-abc");
        assert_eq!(v["request"]["contents"][0]["role"], "user");
        assert_eq!(v["request"]["contents"][0]["parts"][0]["text"], "hi");

        // unwrap: `.response` extracted.
        let enveloped = Bytes::from(r#"{"response":{"candidates":[{"x":1}]}}"#);
        let inner: Value = serde_json::from_slice(&unwrap_code_assist(enveloped)).unwrap();
        assert_eq!(inner["candidates"][0]["x"], 1);

        // unwrap: non-envelope body returned unchanged.
        let plain = Bytes::from(r#"{"candidates":[]}"#);
        assert_eq!(unwrap_code_assist(plain.clone()), plain);

        // stream: per-frame `.response` unwrap.
        let mut dec = CodeAssistStreamDecoder::new();
        let mut out = dec.push(b"data: {\"response\":{\"t\":\"a\"}}\n\n");
        out.extend(dec.finish());
        assert_eq!(String::from_utf8(out).unwrap(), "data: {\"t\":\"a\"}\n\n");
    }
}
