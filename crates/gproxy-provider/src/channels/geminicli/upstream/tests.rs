use serde_json::json;

use super::strip_geminicli_unsupported_generation_config;

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
