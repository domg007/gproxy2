use serde_json::json;

use super::{
    geminicli_response_indicates_quota_exhausted, strip_geminicli_unsupported_generation_config,
};

#[test]
fn strip_geminicli_unsupported_generation_config_removes_logprobs() {
    let mut body = json!({
        "contents": [{"role":"user","parts":[{"text":"hello"}]}],
        "generationConfig": {
            "temperature": 1,
            "logprobs": 5,
            "responseLogprobs": true,
            "maxOutputTokens": 1024
        }
    });

    strip_geminicli_unsupported_generation_config(&mut body);

    assert_eq!(
        body.pointer("/generationConfig/temperature")
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert!(body.pointer("/generationConfig/logprobs").is_none());
    assert!(body.pointer("/generationConfig/responseLogprobs").is_none());
    assert!(body.pointer("/generationConfig/maxOutputTokens").is_none());
}

#[test]
fn geminicli_429_with_quota_error_info_is_detected_as_quota_exhausted() {
    let body = json!({
        "error": {
            "code": 429,
            "status": "RESOURCE_EXHAUSTED",
            "details": [{
                "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                "reason": "QUOTA_EXHAUSTED",
                "metadata": {
                    "quotaResetTimeStamp": "2025-11-30T14:57:24Z"
                }
            }]
        }
    });

    let bytes = serde_json::to_vec(&body).expect("serialize body");
    assert!(geminicli_response_indicates_quota_exhausted(
        bytes.as_slice()
    ));
}

#[test]
fn geminicli_429_without_quota_error_info_is_not_detected_as_quota_exhausted() {
    let body = json!({
        "error": {
            "code": 429,
            "status": "RESOURCE_EXHAUSTED",
            "message": "upstream overloaded"
        }
    });

    let bytes = serde_json::to_vec(&body).expect("serialize body");
    assert!(!geminicli_response_indicates_quota_exhausted(
        bytes.as_slice()
    ));
}
