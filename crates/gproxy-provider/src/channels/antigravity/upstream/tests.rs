use serde_json::json;

use super::{
    AntigravityRequestKind, build_request_body_bytes, explicit_antigravity_session_id,
    prompt_stable_session_id, session_id_for_kind,
};

#[test]
fn antigravity_build_request_strips_max_output_tokens() {
    let body = json!({
        "contents": [{"role":"user","parts":[{"text":"hello"}]}],
        "generationConfig": {
            "maxOutputTokens": 128000,
            "temperature": 1.0
        }
    });
    let kind = AntigravityRequestKind::Forward {
        requires_project: true,
        request_type: Some("agent"),
    };

    let bytes = build_request_body_bytes(
        Some(&body),
        Some("claude-sonnet-4-6"),
        &kind,
        "inductive-autumn-0x7ln",
        None,
    )
    .expect("request body")
    .expect("wrapped bytes");

    let wrapped: serde_json::Value = serde_json::from_slice(&bytes).expect("json body");
    let generation_config = wrapped
        .get("request")
        .and_then(|request| request.get("generationConfig"))
        .and_then(|config| config.as_object())
        .expect("generation config");

    assert!(!generation_config.contains_key("maxOutputTokens"));
    assert_eq!(generation_config.get("temperature"), Some(&json!(1.0)));
}

#[test]
fn antigravity_session_id_prefers_explicit_request_value() {
    let body = json!({
        "sessionId": "sess-explicit",
        "systemInstruction": {"parts":[{"text":"be concise"}]},
        "contents": [{"role":"user","parts":[{"text":"hello"}]}]
    });

    assert_eq!(
        explicit_antigravity_session_id(Some(&body)).as_deref(),
        Some("sess-explicit")
    );
}

#[test]
fn antigravity_prompt_session_uses_system_and_first_user_content_only() {
    let body_a = json!({
        "systemInstruction": {"parts":[{"text":"be concise"}]},
        "contents": [
            {"role":"user","parts":[{"text":"hello"}]},
            {"role":"model","parts":[{"text":"draft one"}]},
            {"role":"user","parts":[{"text":"follow up a"}]}
        ],
        "generationConfig": {"temperature": 0.2}
    });
    let body_b = json!({
        "systemInstruction": {"parts":[{"text":"be concise"}]},
        "contents": [
            {"role":"user","parts":[{"text":"hello"}]},
            {"role":"model","parts":[{"text":"draft two"}]},
            {"role":"user","parts":[{"text":"follow up b"}]}
        ],
        "generationConfig": {"temperature": 0.9}
    });
    let body_c = json!({
        "systemInstruction": {"parts":[{"text":"be concise"}]},
        "contents": [
            {"role":"user","parts":[{"text":"different opener"}]}
        ]
    });

    let session_id_a = prompt_stable_session_id(Some(&body_a)).expect("session id a");
    let session_id_b = prompt_stable_session_id(Some(&body_b)).expect("session id b");
    let session_id_c = prompt_stable_session_id(Some(&body_c)).expect("session id c");

    assert_eq!(session_id_a, session_id_b);
    assert_ne!(session_id_a, session_id_c);
}

#[test]
fn antigravity_session_id_for_kind_returns_none_without_markers() {
    let kind = AntigravityRequestKind::Forward {
        requires_project: true,
        request_type: None,
    };

    assert!(session_id_for_kind(&kind, None).is_none());
}
