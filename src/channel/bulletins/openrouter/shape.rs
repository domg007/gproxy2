//! OpenRouter response shaping (整形).
//!
//! OpenRouter error shape: `{error: {code: int, message: str, metadata?}, ...}`.
//! OpenAI's error model requires `message` + `type` as strings and an optional
//! string `code`. Coerce `code` to string and synthesize `type` from the
//! numeric code so downstream transforms deserialize cleanly. Best-effort:
//! returns the input unchanged on parse failure or when there is no int
//! `error.code`.

use bytes::Bytes;
use serde_json::Value;

/// Map an OpenRouter numeric error code to an OpenAI-style `error.type`.
fn error_type_for(code: i64) -> &'static str {
    match code {
        400 => "invalid_request_error",
        401 => "authentication_error",
        402 => "insufficient_quota",
        403 => "permission_error",
        404 => "not_found_error",
        408 => "timeout_error",
        429 => "rate_limit_error",
        500..=599 => "api_error",
        _ => "api_error",
    }
}

/// Reshape an OpenRouter error body: int `error.code` -> string, synthesize
/// `error.type` from the code if absent. No-op for non-error bodies, bodies
/// whose `error.code` is not an integer, or bodies that fail to parse.
pub(super) fn reshape_error(body: Bytes) -> Bytes {
    let Ok(mut v) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let Some(err) = v.get_mut("error").and_then(Value::as_object_mut) else {
        return body;
    };
    let Some(code) = err.get("code").and_then(Value::as_i64) else {
        return body;
    };
    err.insert("code".to_string(), Value::from(code.to_string()));
    err.entry("type")
        .or_insert_with(|| Value::from(error_type_for(code)));
    serde_json::to_vec(&v).map(Bytes::from).unwrap_or(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn int_code_becomes_string_and_type_synthesized() {
        let body = Bytes::from(
            serde_json::to_vec(&json!({
                "error": { "code": 429, "message": "rate limited" }
            }))
            .unwrap(),
        );
        let out = reshape_error(body);
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["error"]["code"], "429");
        assert_eq!(v["error"]["type"], "rate_limit_error");
        assert_eq!(v["error"]["message"], "rate limited");
    }

    #[test]
    fn existing_type_is_preserved() {
        let body = Bytes::from(
            serde_json::to_vec(&json!({
                "error": { "code": 400, "type": "custom_type", "message": "bad" }
            }))
            .unwrap(),
        );
        let out = reshape_error(body);
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["error"]["code"], "400");
        assert_eq!(v["error"]["type"], "custom_type");
    }

    #[test]
    fn non_error_body_is_untouched() {
        let raw = br#"{"data":[{"id":"x"}],"object":"list"}"#;
        let out = reshape_error(Bytes::from_static(raw));
        assert_eq!(out.as_ref(), raw);
    }

    #[test]
    fn non_int_code_is_untouched() {
        // Already a string code -> leave it (and any absent type) alone.
        let raw = br#"{"error":{"code":"already_str","message":"x"}}"#;
        let out = reshape_error(Bytes::from_static(raw));
        assert_eq!(out.as_ref(), raw);
    }

    #[test]
    fn parse_failure_returns_input() {
        let raw = b"not json at all";
        let out = reshape_error(Bytes::from_static(raw));
        assert_eq!(out.as_ref(), raw);
    }
}
