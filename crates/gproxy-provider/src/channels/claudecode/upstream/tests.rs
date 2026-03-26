use gproxy_middleware::{OperationFamily, ProtocolKind};
use serde_json::json;

use super::{
    CLAUDECODE_THINKING_BUDGET_TOKENS, apply_claudecode_billing_header_system_block,
    apply_claudecode_metadata_user_id_json, ensure_oauth_beta,
    extend_model_list_with_thinking_variants, flatten_system_text_before_cache_control,
    merge_claudecode_beta_headers, normalize_claudecode_model_and_thinking,
    normalize_claudecode_unsupported_fields, prepared::ClaudeCodePreparedRequest,
    strip_context_1m_beta,
};
use crate::channels::cache_control::{
    CacheBreakpointPositionKind, CacheBreakpointRule, CacheBreakpointTarget, CacheBreakpointTtl,
};
use crate::channels::claudecode::constants::{CLAUDECODE_REFERENCE_BETAS, OAUTH_BETA};
use crate::channels::claudecode::oauth::ClaudeCodeAuthMaterial;

#[test]
fn thinking_suffix_sets_fixed_budget_and_strips_model_suffix() {
    let mut body = json!({
        "model": "claude-opus-4-5-thinking",
        "messages": [],
        "max_tokens": 2048
    });

    let model = normalize_claudecode_model_and_thinking("claude-opus-4-5-thinking", &mut body);

    assert_eq!(model, "claude-opus-4-5");
    assert_eq!(body["model"], json!("claude-opus-4-5"));
    assert_eq!(body["thinking"]["type"], json!("enabled"));
    assert_eq!(
        body["thinking"]["budget_tokens"],
        json!(CLAUDECODE_THINKING_BUDGET_TOKENS)
    );
}

#[test]
fn adaptive_thinking_suffix_forces_adaptive() {
    let mut body = json!({
        "model": "claude-opus-4-5-adaptive-thinking",
        "thinking": {
            "type": "enabled",
            "budget_tokens": 1024
        }
    });

    let model =
        normalize_claudecode_model_and_thinking("claude-opus-4-5-adaptive-thinking", &mut body);

    assert_eq!(model, "claude-opus-4-5");
    assert_eq!(body["model"], json!("claude-opus-4-5"));
    assert_eq!(body["thinking"], json!({"type": "adaptive"}));
}

#[test]
fn thinking_suffix_overrides_existing_to_fixed_budget() {
    let mut body = json!({
        "model": "claude-sonnet-4-5-thinking",
        "thinking": {
            "type": "enabled",
            "budget_tokens": 2048
        }
    });

    let model = normalize_claudecode_model_and_thinking("claude-sonnet-4-5-thinking", &mut body);

    assert_eq!(model, "claude-sonnet-4-5");
    assert_eq!(body["model"], json!("claude-sonnet-4-5"));
    assert_eq!(
        body["thinking"],
        json!({
            "type": "enabled",
            "budget_tokens": CLAUDECODE_THINKING_BUDGET_TOKENS
        })
    );
}

#[test]
fn model_list_expands_with_thinking_variants() {
    let mut data = vec![
        json!({"id": "claude-opus-4-6", "object": "model"}),
        json!({"id": "claude-sonnet-4-5", "object": "model"}),
    ];

    extend_model_list_with_thinking_variants(&mut data);

    let ids = data
        .iter()
        .filter_map(|item| item.get("id").and_then(|v| v.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "claude-opus-4-6",
            "claude-opus-4-6-thinking",
            "claude-opus-4-6-adaptive-thinking",
            "claude-sonnet-4-5",
            "claude-sonnet-4-5-thinking",
            "claude-sonnet-4-5-adaptive-thinking"
        ]
    );
}

#[test]
fn model_list_does_not_duplicate_existing_thinking_entries() {
    let mut data = vec![
        json!({"id": "claude-opus-4-6", "object": "model"}),
        json!({"id": "claude-opus-4-6-thinking", "object": "model"}),
    ];

    extend_model_list_with_thinking_variants(&mut data);

    let mut ids = data
        .iter()
        .filter_map(|item| item.get("id").and_then(|v| v.as_str()))
        .collect::<Vec<_>>();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec![
            "claude-opus-4-6",
            "claude-opus-4-6-adaptive-thinking",
            "claude-opus-4-6-thinking",
        ]
    );
}

#[test]
fn normalize_claudecode_unsupported_fields_preserves_context_management() {
    let mut body = json!({
        "model": "claude-sonnet-4-5",
        "context_management": {
            "edits": [{
                "type": "compact_20260112"
            }]
        },
        "messages": []
    });

    normalize_claudecode_unsupported_fields(&mut body);

    assert!(body.get("context_management").is_some());
}

#[test]
fn normalize_claudecode_unsupported_fields_removes_speed() {
    let mut body = json!({
        "model": "claude-opus-4-6",
        "speed": "fast",
        "messages": []
    });

    normalize_claudecode_unsupported_fields(&mut body);

    assert!(body.get("speed").is_none());
}

#[test]
fn ensure_oauth_beta_keeps_custom_and_only_adds_required_oauth_beta() {
    let mut headers = vec![(
        "anthropic-beta".to_string(),
        "custom-beta,effort-2025-11-24".to_string(),
    )];

    ensure_oauth_beta(&mut headers, false);

    assert_eq!(
        headers,
        vec![(
            "anthropic-beta".to_string(),
            ["custom-beta", "effort-2025-11-24", OAUTH_BETA].join(","),
        )]
    );
}

#[test]
fn merge_claudecode_beta_headers_puts_selected_values_in_front() {
    let mut headers = vec![(
        "anthropic-beta".to_string(),
        "custom-beta,oauth-2025-04-20".to_string(),
    )];

    merge_claudecode_beta_headers(
        &mut headers,
        &[
            CLAUDECODE_REFERENCE_BETAS[1].to_string(),
            CLAUDECODE_REFERENCE_BETAS[0].to_string(),
        ],
        false,
    );

    assert_eq!(
        headers,
        vec![(
            "anthropic-beta".to_string(),
            [
                CLAUDECODE_REFERENCE_BETAS[1],
                CLAUDECODE_REFERENCE_BETAS[0],
                "custom-beta",
                OAUTH_BETA,
            ]
            .join(","),
        )]
    );
}

#[test]
fn strip_context_1m_beta_keeps_custom_beta_and_oauth() {
    let mut headers = vec![(
        "anthropic-beta".to_string(),
        "context-1m-2025-08-07,custom-beta".to_string(),
    )];

    strip_context_1m_beta(&mut headers);

    assert_eq!(
        headers,
        vec![(
            "anthropic-beta".to_string(),
            ["custom-beta", OAUTH_BETA].join(","),
        )]
    );
}

fn sample_auth_material() -> ClaudeCodeAuthMaterial {
    ClaudeCodeAuthMaterial {
        access_token: "token".to_string(),
        refresh_token: "refresh".to_string(),
        expires_at_unix_ms: 0,
        cookie: None,
        subscription_type: None,
        rate_limit_tier: None,
        user_email: Some("user@example.com".to_string()),
        account_uuid: Some("acct_123".to_string()),
        organization_uuid: Some("org_123".to_string()),
    }
}

#[test]
fn injects_metadata_user_id_when_missing() {
    let mut body = json!({
        "system": "system prompt",
        "messages": [
            {"role":"user","content":"hello world"}
        ]
    });

    apply_claudecode_metadata_user_id_json(&mut body, 7, &sample_auth_material());

    let user_id = body["metadata"]["user_id"].as_str().expect("user_id");
    let parsed: serde_json::Value = serde_json::from_str(user_id).expect("json");
    assert_eq!(parsed["account_uuid"], json!("acct_123"));
    assert!(
        parsed["device_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(
        parsed["session_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
}

#[test]
fn preserves_existing_metadata_user_id() {
    let mut body = json!({
        "metadata": {
            "user_id": "{\"device_id\":\"x\",\"account_uuid\":\"y\",\"session_id\":\"z\"}"
        },
        "messages": [
            {"role":"user","content":"hello world"}
        ]
    });

    apply_claudecode_metadata_user_id_json(&mut body, 7, &sample_auth_material());

    assert_eq!(
        body["metadata"]["user_id"].as_str(),
        Some("{\"device_id\":\"x\",\"account_uuid\":\"y\",\"session_id\":\"z\"}")
    );
}

#[test]
fn prepared_request_skips_beta_query_when_disabled() {
    let payload = serde_json::to_vec(&json!({
        "body": {
            "model": "claude-sonnet-4-5",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "hi"}]
        }
    }))
    .expect("serialize payload");

    let prepared = ClaudeCodePreparedRequest::from_payload(
        OperationFamily::GenerateContent,
        ProtocolKind::Claude,
        payload.as_slice(),
        false,
        None,
        false,
        &[],
    )
    .expect("prepare payload");

    assert_eq!(prepared.path, "/v1/messages");
}

#[test]
fn prepared_request_appends_beta_query_when_enabled() {
    let payload = serde_json::to_vec(&json!({
        "body": {
            "model": "claude-sonnet-4-5",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "hi"}]
        }
    }))
    .expect("serialize payload");

    let prepared = ClaudeCodePreparedRequest::from_payload(
        OperationFamily::GenerateContent,
        ProtocolKind::Claude,
        payload.as_slice(),
        true,
        None,
        false,
        &[],
    )
    .expect("prepare payload");

    assert_eq!(prepared.path, "/v1/messages?beta=true");
}

#[test]
fn prepared_request_preserves_explicit_context_1m_beta() {
    let payload = serde_json::to_vec(&json!({
        "headers": {
            "anthropic-beta": ["context-1m-2025-08-07"]
        },
        "body": {
            "model": "claude-opus-4-6",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "hi"}]
        }
    }))
    .expect("serialize payload");

    let prepared = ClaudeCodePreparedRequest::from_payload(
        OperationFamily::GenerateContent,
        ProtocolKind::Claude,
        payload.as_slice(),
        false,
        None,
        false,
        &[],
    )
    .expect("prepare payload");

    assert_eq!(
        prepared.request_headers,
        vec![
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
            (
                "anthropic-beta".to_string(),
                ["context-1m-2025-08-07", OAUTH_BETA].join(","),
            ),
        ]
    );
}

#[test]
fn prepared_request_preserves_flat_string_anthropic_beta_values() {
    let payload = serde_json::to_vec(&json!({
        "headers": {
            "anthropic-beta": "output-128k-2025-02-19,context-1m-2025-08-07,context-management-2025-06-27,compact-2026-01-12"
        },
        "body": {
            "model": "claude-opus-4-6",
            "max_tokens": 32,
            "messages": [{"role": "user", "content": "hi"}]
        }
    }))
    .expect("serialize payload");

    let prepared = ClaudeCodePreparedRequest::from_payload(
        OperationFamily::GenerateContent,
        ProtocolKind::Claude,
        payload.as_slice(),
        false,
        None,
        false,
        &[],
    )
    .expect("prepare payload");

    assert_eq!(
        prepared.request_headers,
        vec![
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
            (
                "anthropic-beta".to_string(),
                [
                    "output-128k-2025-02-19",
                    "context-1m-2025-08-07",
                    "context-management-2025-06-27",
                    "compact-2026-01-12",
                    OAUTH_BETA,
                ]
                .join(","),
            ),
        ]
    );
}

#[test]
fn prepared_request_canonicalizes_claude_shorthand_content_blocks() {
    let payload = serde_json::to_vec(&json!({
        "body": {
            "model": "claude-sonnet-4-5",
            "max_tokens": 32,
            "system": "sys",
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": {"type": "text", "text": "there"}}
            ]
        }
    }))
    .expect("serialize payload");

    let prepared = ClaudeCodePreparedRequest::from_payload(
        OperationFamily::GenerateContent,
        ProtocolKind::Claude,
        payload.as_slice(),
        false,
        None,
        false,
        &[],
    )
    .expect("prepare payload");

    let body: serde_json::Value =
        serde_json::from_slice(prepared.body.as_deref().expect("body bytes")).expect("valid json");
    assert_eq!(
        body["system"][0]["text"],
        json!("x-anthropic-billing-header: cc_version=2.1.76.4dc; cc_entrypoint=cli; cch=00000;")
    );
    assert_eq!(body["system"][1]["text"], json!("sys"));
    assert_eq!(body["messages"][0]["content"][0]["text"], json!("hi"));
    assert_eq!(body["messages"][1]["content"][0]["text"], json!("there"));
}

#[test]
fn billing_header_uses_dynamic_version_hash_and_fixed_cch() {
    let mut body = json!({
        "model": "claude-sonnet-4-5",
        "messages": [
            {"role": "user", "content": "hey"}
        ]
    });

    apply_claudecode_billing_header_system_block(&mut body);

    assert_eq!(
        body["system"][0]["text"],
        json!("x-anthropic-billing-header: cc_version=2.1.76.4dc; cc_entrypoint=cli; cch=00000;")
    );
}

#[test]
fn billing_header_preserves_existing_system_block() {
    let mut body = json!({
        "model": "claude-sonnet-4-5",
        "system": [{
            "type": "text",
            "text": "x-anthropic-billing-header: cc_version=custom.keep; cc_entrypoint=cli; cch=12345;"
        }],
        "messages": [
            {"role": "user", "content": "hey"}
        ]
    });

    apply_claudecode_billing_header_system_block(&mut body);

    assert_eq!(
        body["system"],
        json!([{
            "type": "text",
            "text": "x-anthropic-billing-header: cc_version=custom.keep; cc_entrypoint=cli; cch=12345;"
        }])
    );
}

#[test]
fn prepared_request_inserts_billing_header_after_cache_rules() {
    let payload = serde_json::to_vec(&json!({
        "body": {
            "model": "claude-sonnet-4-5",
            "max_tokens": 32,
            "system": "sys",
            "messages": [{"role": "user", "content": "hey"}]
        }
    }))
    .expect("serialize payload");

    let prepared = ClaudeCodePreparedRequest::from_payload(
        OperationFamily::GenerateContent,
        ProtocolKind::Claude,
        payload.as_slice(),
        false,
        None,
        false,
        &[CacheBreakpointRule {
            target: CacheBreakpointTarget::System,
            position: CacheBreakpointPositionKind::Nth,
            index: 1,
            content_position: None,
            content_index: None,
            ttl: CacheBreakpointTtl::Ttl5m,
        }],
    )
    .expect("prepare payload");

    let body: serde_json::Value =
        serde_json::from_slice(prepared.body.as_deref().expect("body bytes")).expect("valid json");
    assert_eq!(
        body["system"][0]["text"],
        json!("x-anthropic-billing-header: cc_version=2.1.76.4dc; cc_entrypoint=cli; cch=00000;")
    );
    assert!(body["system"][0].get("cache_control").is_none());
    assert_eq!(body["system"][1]["text"], json!("sys"));
    assert_eq!(
        body["system"][1]["cache_control"],
        json!({
            "type": "ephemeral",
            "ttl": "5m"
        })
    );
}

#[test]
fn prepared_request_keeps_existing_billing_header() {
    let payload = serde_json::to_vec(&json!({
        "body": {
            "model": "claude-sonnet-4-5",
            "max_tokens": 32,
            "system": [{
                "type": "text",
                "text": "x-anthropic-billing-header: cc_version=already.there; cc_entrypoint=cli; cch=99999;"
            }, {
                "type": "text",
                "text": "sys"
            }],
            "messages": [{"role": "user", "content": "hey"}]
        }
    }))
    .expect("serialize payload");

    let prepared = ClaudeCodePreparedRequest::from_payload(
        OperationFamily::GenerateContent,
        ProtocolKind::Claude,
        payload.as_slice(),
        false,
        None,
        false,
        &[],
    )
    .expect("prepare payload");

    let body: serde_json::Value =
        serde_json::from_slice(prepared.body.as_deref().expect("body bytes")).expect("valid json");
    assert_eq!(
        body["system"][0]["text"],
        json!(
            "x-anthropic-billing-header: cc_version=already.there; cc_entrypoint=cli; cch=99999;"
        )
    );
    assert_eq!(body["system"][1]["text"], json!("sys"));
}

#[test]
fn flatten_system_text_before_cache_control_merges_text_blocks_before_first_cache_point() {
    let mut body = json!({
        "system": [
            {"type":"text","text":"a"},
            {"type":"text","text":"b"},
            {"type":"text","text":"c","cache_control":{"type":"ephemeral","ttl":"5m"}},
            {"type":"text","text":"d"}
        ]
    });

    flatten_system_text_before_cache_control(&mut body);

    assert_eq!(
        body["system"],
        json!([
            {"type":"text","text":"ab"},
            {"type":"text","text":"c","cache_control":{"type":"ephemeral","ttl":"5m"}},
            {"type":"text","text":"d"}
        ])
    );
}

#[test]
fn flatten_system_text_before_cache_control_preserves_leading_billing_header() {
    let mut body = json!({
        "system": [
            {
                "type":"text",
                "text":"x-anthropic-billing-header: cc_version=custom.keep; cc_entrypoint=cli; cch=12345;"
            },
            {"type":"text","text":"a"},
            {"type":"text","text":"b"},
            {"type":"text","text":"c","cache_control":{"type":"ephemeral","ttl":"5m"}}
        ]
    });

    flatten_system_text_before_cache_control(&mut body);

    assert_eq!(
        body["system"],
        json!([
            {
                "type":"text",
                "text":"x-anthropic-billing-header: cc_version=custom.keep; cc_entrypoint=cli; cch=12345;"
            },
            {"type":"text","text":"ab"},
            {"type":"text","text":"c","cache_control":{"type":"ephemeral","ttl":"5m"}}
        ])
    );
}

#[test]
fn flatten_system_text_before_cache_control_handles_multiple_cache_points() {
    let mut body = json!({
        "system": [
            {"type":"text","text":"a"},
            {"type":"text","text":"b"},
            {"type":"text","text":"c","cache_control":{"type":"ephemeral","ttl":"5m"}},
            {"type":"text","text":"d"},
            {"type":"text","text":"e"},
            {"type":"text","text":"f","cache_control":{"type":"ephemeral","ttl":"1h"}},
            {"type":"text","text":"g"}
        ]
    });

    flatten_system_text_before_cache_control(&mut body);

    assert_eq!(
        body["system"],
        json!([
            {"type":"text","text":"ab"},
            {"type":"text","text":"c","cache_control":{"type":"ephemeral","ttl":"5m"}},
            {"type":"text","text":"de"},
            {"type":"text","text":"f","cache_control":{"type":"ephemeral","ttl":"1h"}},
            {"type":"text","text":"g"}
        ])
    );
}

#[test]
fn prepared_request_flattens_system_text_before_cache_control_when_enabled() {
    let payload = serde_json::to_vec(&json!({
        "body": {
            "model": "claude-sonnet-4-5",
            "max_tokens": 32,
            "system": [
                {"type": "text", "text": "sys-a"},
                {"type": "text", "text": "sys-b"},
                {"type": "text", "text": "sys-c"}
            ],
            "messages": [{"role": "user", "content": "hey"}]
        }
    }))
    .expect("serialize payload");

    let prepared = ClaudeCodePreparedRequest::from_payload(
        OperationFamily::GenerateContent,
        ProtocolKind::Claude,
        payload.as_slice(),
        false,
        None,
        true,
        &[CacheBreakpointRule {
            target: CacheBreakpointTarget::System,
            position: CacheBreakpointPositionKind::Nth,
            index: 3,
            content_position: None,
            content_index: None,
            ttl: CacheBreakpointTtl::Ttl5m,
        }],
    )
    .expect("prepare payload");

    let body: serde_json::Value =
        serde_json::from_slice(prepared.body.as_deref().expect("body bytes")).expect("valid json");
    assert_eq!(
        body["system"][0]["text"],
        json!("x-anthropic-billing-header: cc_version=2.1.76.4dc; cc_entrypoint=cli; cch=00000;")
    );
    assert_eq!(body["system"][1]["text"], json!("sys-asys-b"));
    assert_eq!(body["system"][2]["text"], json!("sys-c"));
    assert_eq!(
        body["system"][2]["cache_control"],
        json!({
            "type": "ephemeral",
            "ttl": "5m"
        })
    );
}
