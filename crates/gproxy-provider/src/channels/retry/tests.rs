use serde_json::json;

use super::affinity::{
    build_prefix_hashes, cache_affinity_hint_for_claude_effective_body,
    cache_affinity_hint_for_codex_openai_responses, cache_affinity_hint_for_gemini,
    cache_affinity_hint_for_openai_chat, cache_affinity_hint_for_openai_responses, hash_str_to_hex,
    non_claude_candidate_indices, openai_chat_cache_blocks,
};
use super::selection::{
    bind_affinity, clear_affinity, get_affinity_credential_id, pick_candidate_index,
};
use super::{
    CredentialCandidate, CredentialPickMode, OPENAI_24H_CACHE_AFFINITY_TTL_MS,
    ScopedAffinityCandidate,
};

const DEFAULT_MAX_AFFINITY_KEYS: usize = 4096;

#[test]
fn openai_chat_ignores_stream_and_sampling_for_affinity() {
    let body_a = json!({
        "model": "gpt-5",
        "prompt_cache_key": "k1",
        "stream": false,
        "temperature": 0.1,
        "max_tokens": 200,
        "tools": [{"type":"function","function":{"name":"f"}}],
        "messages": [{"role":"user","content":"hello"}],
    });
    let body_b = json!({
        "model": "gpt-5",
        "prompt_cache_key": "k1",
        "stream": true,
        "temperature": 0.9,
        "max_tokens": 999,
        "tools": [{"type":"function","function":{"name":"f"}}],
        "messages": [{"role":"user","content":"hello"}],
    });

    let hint_a = cache_affinity_hint_for_openai_chat(body_a).expect("hint a");
    let hint_b = cache_affinity_hint_for_openai_chat(body_b).expect("hint b");
    assert_eq!(hint_a.bind.key, hint_b.bind.key);
}

#[test]
fn openai_responses_ignores_stream_and_output_tokens_for_affinity() {
    let body_a = json!({
        "model": "gpt-5",
        "stream": false,
        "max_output_tokens": 128,
        "input": [{"role":"user","content":[{"type":"input_text","text":"hello"}]}],
    });
    let body_b = json!({
        "model": "gpt-5",
        "stream": true,
        "max_output_tokens": 4096,
        "input": [{"role":"user","content":[{"type":"input_text","text":"hello"}]}],
    });

    let hint_a = cache_affinity_hint_for_openai_responses(body_a).expect("hint a");
    let hint_b = cache_affinity_hint_for_openai_responses(body_b).expect("hint b");
    assert_eq!(hint_a.bind.key, hint_b.bind.key);
}

#[test]
fn codex_openai_responses_uses_prompt_cache_key_as_session_marker() {
    let body = json!({
        "model": "gpt-5.3-codex",
        "prompt_cache_key": "thread-123",
        "input": [{"role":"user","content":[{"type":"input_text","text":"hello"}]}]
    });
    let bytes = serde_json::to_vec(&body).expect("serialize body");

    let hint =
        cache_affinity_hint_for_codex_openai_responses(Some(bytes.as_slice())).expect("codex hint");
    let expected = format!("codex.responses.session:{}", hash_str_to_hex("thread-123"));
    assert_eq!(hint.bind.key, expected);
    assert_eq!(hint.bind.ttl_ms, OPENAI_24H_CACHE_AFFINITY_TTL_MS);
    assert_eq!(hint.candidates.len(), 1);
    assert_eq!(hint.candidates[0].key, hint.bind.key);
}

#[test]
fn codex_openai_responses_falls_back_to_conversation_and_previous_response() {
    let conversation_body = json!({
        "model": "gpt-5.3-codex",
        "conversation": { "id": "conv-abc" }
    });
    let conversation_bytes =
        serde_json::to_vec(&conversation_body).expect("serialize conversation body");
    let conversation_hint =
        cache_affinity_hint_for_codex_openai_responses(Some(conversation_bytes.as_slice()))
            .expect("conversation hint");
    assert_eq!(
        conversation_hint.bind.key,
        format!("codex.responses.session:{}", hash_str_to_hex("conv-abc"))
    );

    let previous_body = json!({
        "model": "gpt-5.3-codex",
        "previous_response_id": "resp_42"
    });
    let previous_bytes = serde_json::to_vec(&previous_body).expect("serialize previous body");
    let previous_hint =
        cache_affinity_hint_for_codex_openai_responses(Some(previous_bytes.as_slice()))
            .expect("previous response hint");
    assert_eq!(
        previous_hint.bind.key,
        format!("codex.responses.session:{}", hash_str_to_hex("resp_42"))
    );
}

#[test]
fn claude_without_breakpoints_returns_none() {
    let body = json!({
        "model": "claude-sonnet-4-6",
        "messages": [{"role":"user","content":"hello"}]
    });
    assert!(cache_affinity_hint_for_claude_effective_body(body).is_none());
}

#[test]
fn claude_top_level_cache_control_creates_auto_breakpoint() {
    let body = json!({
        "model": "claude-sonnet-4-6",
        "cache_control": {"type":"ephemeral", "ttl":"1h"},
        "messages": [{"role":"user","content":"hello"}]
    });
    let hint = cache_affinity_hint_for_claude_effective_body(body).expect("hint");
    assert!(hint.bind.key.contains("bp=auto"));
    assert!(hint.bind.key.contains("ttl=1h"));
}

#[test]
fn claude_top_level_cache_control_without_ttl_defaults_to_5m_in_generic_path() {
    let body = json!({
        "model": "claude-sonnet-4-6",
        "cache_control": {"type":"ephemeral"},
        "messages": [{"role":"user","content":"hello"}]
    });
    let hint = cache_affinity_hint_for_claude_effective_body(body).expect("hint");
    assert!(hint.bind.key.contains("bp=auto"));
    assert!(hint.bind.key.contains("ttl=5m"));
}

#[test]
fn claude_explicit_breakpoint_creates_candidates() {
    let body = json!({
        "model": "claude-sonnet-4-6",
        "messages": [{
            "role":"user",
            "content":[{"type":"text","text":"hello","cache_control":{"type":"ephemeral"}}]
        }]
    });
    let hint = cache_affinity_hint_for_claude_effective_body(body).expect("hint");
    assert!(!hint.candidates.is_empty());
    assert!(hint.bind.key.contains("bp=explicit"));
    assert!(hint.bind.key.contains("ttl=5m"));
}

#[test]
fn claude_explicit_breakpoint_with_5m_ttl_stays_5m() {
    let body = json!({
        "model": "claude-sonnet-4-6",
        "messages": [{
            "role":"user",
            "content":[{"type":"text","text":"hello","cache_control":{"type":"ephemeral","ttl":"5m"}}]
        }]
    });
    let hint = cache_affinity_hint_for_claude_effective_body(body).expect("hint");
    assert!(!hint.candidates.is_empty());
    assert!(hint.bind.key.contains("bp=explicit"));
    assert!(hint.bind.key.contains("ttl=5m"));
}

#[test]
fn claude_shorthand_and_canonical_blocks_hash_identically() {
    let shorthand = json!({
        "model": "claude-sonnet-4-6",
        "cache_control": {"type":"ephemeral", "ttl":"1h"},
        "messages": [{"role":"user","content":"hello"}]
    });
    let canonical = json!({
        "model": "claude-sonnet-4-6",
        "cache_control": {"type":"ephemeral", "ttl":"1h"},
        "messages": [{
            "role":"user",
            "content":[{"type":"text","text":"hello"}]
        }]
    });

    let shorthand_hint = cache_affinity_hint_for_claude_effective_body(shorthand).expect("hint");
    let canonical_hint = cache_affinity_hint_for_claude_effective_body(canonical).expect("hint");

    assert_eq!(shorthand_hint.bind.key, canonical_hint.bind.key);
}

#[test]
fn claude_top_level_cache_control_without_ttl_defaults_to_5m() {
    let body = json!({
        "model": "claude-sonnet-4-6",
        "cache_control": {"type":"ephemeral"},
        "messages": [{"role":"user","content":"hello"}]
    });
    let hint = cache_affinity_hint_for_claude_effective_body(body).expect("hint");

    assert!(hint.bind.key.contains("bp=auto"));
    assert!(hint.bind.key.contains("ttl=5m"));
}

#[test]
fn claude_explicit_breakpoint_without_ttl_defaults_to_5m() {
    let body = json!({
        "model": "claude-sonnet-4-6",
        "messages": [{
            "role":"user",
            "content":[{"type":"text","text":"hello","cache_control":{"type":"ephemeral"}}]
        }]
    });
    let hint = cache_affinity_hint_for_claude_effective_body(body).expect("hint");

    assert!(hint.bind.key.contains("bp=explicit"));
    assert!(hint.bind.key.contains("ttl=5m"));
}

#[test]
fn gemini_cached_content_uses_strong_key() {
    let body = json!({
        "cachedContent": "cachedContents/abc",
        "contents": [{"role":"user","parts":[{"text":"hello"}]}]
    });
    let hint = cache_affinity_hint_for_gemini("models/gemini-2.5-pro", body).expect("hint");
    assert!(hint.bind.key.starts_with("gemini.cachedContent:"));
    assert_eq!(hint.candidates.len(), 1);
}

#[test]
fn gemini_prefix_mode_when_no_cached_content() {
    let body = json!({
        "systemInstruction": {"role":"system","parts":[{"text":"s"}]},
        "contents": [{"role":"user","parts":[{"text":"hello"}]}]
    });
    let hint = cache_affinity_hint_for_gemini("models/gemini-2.5-pro", body).expect("hint");
    assert!(hint.bind.key.starts_with("gemini.generateContent:prefix:"));
    assert!(!hint.candidates.is_empty());
}

#[test]
fn non_claude_candidate_sampling_prefers_tail_when_prefixes_exceed_limit() {
    let messages = (0..80)
        .map(|idx| {
            json!({
                "role": "user",
                "content": format!("msg-{idx}")
            })
        })
        .collect::<Vec<_>>();
    let body = json!({
        "model": "gpt-5",
        "prompt_cache_key": "sample-key",
        "messages": messages,
    });

    let hint = cache_affinity_hint_for_openai_chat(body.clone()).expect("hint");
    assert_eq!(hint.candidates.len(), 64);
    assert_eq!(
        hint.candidates.first().map(|v| &v.key),
        Some(&hint.bind.key)
    );

    let blocks = openai_chat_cache_blocks(&body);
    let prefix_hashes = build_prefix_hashes("openai.chat:gpt-5", &blocks).expect("prefix hashes");
    let sampled = non_claude_candidate_indices(prefix_hashes.len());
    assert_eq!(sampled.len(), 64);
    assert_eq!(sampled[0], 79);
    assert_eq!(sampled[55], 24);
    assert_eq!(sampled[56], 7);
    assert_eq!(sampled[63], 0);

    let prompt_cache_key_hash = hash_str_to_hex("sample-key");
    let key_for_index = |idx: usize| {
        format!(
            "openai.chat:ret=in-memory:k={prompt_cache_key_hash}:h={}",
            prefix_hashes[idx]
        )
    };

    assert_eq!(hint.candidates[55].key, key_for_index(24));
    assert_eq!(hint.candidates[56].key, key_for_index(7));
    assert_eq!(hint.candidates[63].key, key_for_index(0));
}

#[test]
fn block_hashes_do_not_cascade_when_middle_block_changes() {
    let blocks_a = vec![
        json!({ "kind": "msg", "value": "a" }),
        json!({ "kind": "msg", "value": "b" }),
        json!({ "kind": "msg", "value": "c" }),
    ];
    let blocks_b = vec![
        json!({ "kind": "msg", "value": "a" }),
        json!({ "kind": "msg", "value": "x" }),
        json!({ "kind": "msg", "value": "c" }),
    ];

    let hashes_a = build_prefix_hashes("seed", &blocks_a).expect("hashes a");
    let hashes_b = build_prefix_hashes("seed", &blocks_b).expect("hashes b");

    assert_eq!(hashes_a.len(), 3);
    assert_eq!(hashes_b.len(), 3);
    assert_eq!(hashes_a[0], hashes_b[0]);
    assert_ne!(hashes_a[1], hashes_b[1]);
    assert_eq!(hashes_a[2], hashes_b[2]);
}

#[test]
fn round_robin_with_cache_uses_sum_of_hit_key_lengths() {
    let now_unix_ms = 1_000_000u64;
    let key_1 = "test::sum-hit::key1";
    let key_2 = "test::sum-hit::key2";
    let key_3 = "test::sum-hit::key3";

    bind_affinity(
        "test",
        key_1,
        101,
        now_unix_ms + 60_000,
        now_unix_ms,
        DEFAULT_MAX_AFFINITY_KEYS,
    );
    bind_affinity(
        "test",
        key_2,
        101,
        now_unix_ms + 60_000,
        now_unix_ms,
        DEFAULT_MAX_AFFINITY_KEYS,
    );
    bind_affinity(
        "test",
        key_3,
        202,
        now_unix_ms + 60_000,
        now_unix_ms,
        DEFAULT_MAX_AFFINITY_KEYS,
    );

    let remaining = vec![
        CredentialCandidate {
            credential_id: 101,
            material: (),
        },
        CredentialCandidate {
            credential_id: 202,
            material: (),
        },
    ];
    let scoped_candidates = vec![
        ScopedAffinityCandidate {
            scoped_key: key_1.to_string(),
            ttl_ms: 60_000,
            key_len: 9,
        },
        ScopedAffinityCandidate {
            scoped_key: key_2.to_string(),
            ttl_ms: 60_000,
            key_len: 9,
        },
        ScopedAffinityCandidate {
            scoped_key: key_3.to_string(),
            ttl_ms: 60_000,
            key_len: 12,
        },
    ];

    let (picked_idx, matched_idx) = pick_candidate_index(
        &remaining,
        &scoped_candidates,
        now_unix_ms,
        CredentialPickMode::RoundRobinWithCache,
    );

    assert_eq!(picked_idx, 0);
    assert_eq!(matched_idx, Some(0));

    clear_affinity(key_1);
    clear_affinity(key_2);
    clear_affinity(key_3);
}

#[test]
fn round_robin_with_cache_scans_candidates_after_miss() {
    let now_unix_ms = 2_000_000u64;
    let key_1 = "test::ordered::key1";
    let key_2 = "test::ordered::key2";
    let key_3 = "test::ordered::key3";

    // key_1 and key_3 exist, key_2 is intentionally missing.
    bind_affinity(
        "test",
        key_1,
        101,
        now_unix_ms + 60_000,
        now_unix_ms,
        DEFAULT_MAX_AFFINITY_KEYS,
    );
    bind_affinity(
        "test",
        key_3,
        202,
        now_unix_ms + 60_000,
        now_unix_ms,
        DEFAULT_MAX_AFFINITY_KEYS,
    );

    let remaining = vec![
        CredentialCandidate {
            credential_id: 101,
            material: (),
        },
        CredentialCandidate {
            credential_id: 202,
            material: (),
        },
    ];
    let scoped_candidates = vec![
        ScopedAffinityCandidate {
            scoped_key: key_1.to_string(),
            ttl_ms: 60_000,
            key_len: 10,
        },
        ScopedAffinityCandidate {
            scoped_key: key_2.to_string(),
            ttl_ms: 60_000,
            key_len: 10,
        },
        ScopedAffinityCandidate {
            scoped_key: key_3.to_string(),
            ttl_ms: 60_000,
            key_len: 100,
        },
    ];

    let (picked_idx, matched_idx) = pick_candidate_index(
        &remaining,
        &scoped_candidates,
        now_unix_ms,
        CredentialPickMode::RoundRobinWithCache,
    );

    // key_3 is still considered even though key_2 misses.
    assert_eq!(picked_idx, 1);
    assert_eq!(matched_idx, Some(2));

    clear_affinity(key_1);
    clear_affinity(key_3);
}

#[test]
fn bind_affinity_enforces_per_channel_key_limit() {
    let now_unix_ms = 3_000_000u64;
    let key_1 = "limit-channel::affinity::key1";
    let key_2 = "limit-channel::affinity::key2";
    let key_3 = "limit-channel::affinity::key3";

    bind_affinity(
        "limit-channel",
        key_1,
        101,
        now_unix_ms + 10_000,
        now_unix_ms,
        2,
    );
    bind_affinity(
        "limit-channel",
        key_2,
        202,
        now_unix_ms + 20_000,
        now_unix_ms,
        2,
    );
    bind_affinity(
        "limit-channel",
        key_3,
        303,
        now_unix_ms + 30_000,
        now_unix_ms,
        2,
    );

    assert_eq!(get_affinity_credential_id(key_1, now_unix_ms), None);
    assert_eq!(get_affinity_credential_id(key_2, now_unix_ms), Some(202));
    assert_eq!(get_affinity_credential_id(key_3, now_unix_ms), Some(303));

    clear_affinity(key_1);
    clear_affinity(key_2);
    clear_affinity(key_3);
}
