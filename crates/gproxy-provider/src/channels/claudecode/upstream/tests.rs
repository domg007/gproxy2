use serde_json::json;

use super::{
    CLAUDECODE_THINKING_BUDGET_TOKENS, ensure_oauth_beta, extend_model_list_with_thinking_variants,
    merge_claudecode_beta_headers, normalize_claudecode_model_and_thinking,
    normalize_claudecode_unsupported_fields, strip_context_1m_beta,
};
use crate::channels::claudecode::constants::{OAUTH_BETA, CLAUDECODE_REFERENCE_BETAS};

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
fn normalize_claudecode_unsupported_fields_removes_context_management() {
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

    assert!(body.get("context_management").is_none());
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
