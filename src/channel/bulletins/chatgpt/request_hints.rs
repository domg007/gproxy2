//! `system_hints` + `thinking_effort` extraction for the `/f/conversation`
//! body. Split out of `request_builder` to keep each file under the size cap.
//! Ported verbatim from v1 `channels/chatgpt/request_builder.rs`.

use serde_json::Value;

/// Pull `system_hints` from the request body.
///
/// Sources, in order (later sources extend earlier ones):
/// 1. `body.system_hints: ["picture_v2", "search", ...]` — upstream-native ids
/// 2. `body.extra_body.system_hints: [...]`
/// 3. `body.tools: [{type: "image_generation" | "web_search_preview" |
///    "deep_research"}]` — standard OpenAI Responses shape, translated to
///    the matching chatgpt-web `system_hint`. Lets the same rewrite_rules
///    preset drive codex/openai upstreams (which accept the tool natively)
///    and chatgpt (which needs the hint form).
pub fn extract_system_hints(body: &Value) -> Vec<String> {
    let mut hints: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        if !s.is_empty() && !hints.iter().any(|x| x == s) {
            hints.push(s.to_string());
        }
    };

    for arr_path in [
        body.get("system_hints"),
        body.get("extra_body").and_then(|x| x.get("system_hints")),
    ] {
        if let Some(arr) = arr_path.and_then(|v| v.as_array()) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    push(s);
                }
            }
        }
    }

    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        for t in tools {
            if let Some(ty) = t.get("type").and_then(|v| v.as_str())
                && let Some(id) = openai_tool_to_hint(ty)
            {
                push(id);
            }
        }
    }

    hints
}

/// Map a standard OpenAI Responses-API tool `type` to its chatgpt-web
/// `system_hint` equivalent. Returns `None` for tools chatgpt-web doesn't
/// have a first-class hint for — those are silently dropped on the chatgpt
/// channel (other channels still forward them natively).
fn openai_tool_to_hint(tool_type: &str) -> Option<&'static str> {
    match tool_type {
        "image_generation" => Some("picture_v2"),
        "web_search" | "web_search_preview" | "web_search_preview_2025_03_11" => Some("search"),
        "deep_research" => Some("connector:connector_openai_deep_research"),
        _ => None,
    }
}

/// Pull a `thinking_effort` value out of an OpenAI-shaped request body.
///
/// Looked up in this priority order:
/// 1. Top-level `thinking_effort` (chatgpt-web native).
/// 2. `extra_body.thinking_effort` (the standard extra-body escape hatch).
/// 3. `extra_body.reasoning.effort` and `reasoning.effort`
///    (OpenAI Responses-API field). Mapped: `low`→`standard`,
///    `medium`→`extended`, `high`→`max`. Other values pass through
///    unchanged so callers can specify the chatgpt-native names directly.
pub fn extract_thinking_effort(body: &Value) -> Option<String> {
    let raw = body
        .get("thinking_effort")
        .and_then(|v| v.as_str())
        .or_else(|| {
            body.get("extra_body")
                .and_then(|x| x.get("thinking_effort"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            body.get("extra_body")
                .and_then(|x| x.get("reasoning"))
                .and_then(|r| r.get("effort"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            body.get("reasoning")
                .and_then(|r| r.get("effort"))
                .and_then(|v| v.as_str())
        })?;
    Some(map_effort(raw).to_string())
}

fn map_effort(raw: &str) -> &str {
    match raw {
        "low" => "standard",
        "medium" => "extended",
        "high" => "max",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::super::request_builder::build_conversation_body;
    use serde_json::json;

    #[test]
    fn forwards_thinking_effort_top_level() {
        let body = json!({
            "model": "gpt-5-thinking",
            "messages": [{"role": "user", "content": "hi"}],
            "thinking_effort": "max"
        });
        let out = build_conversation_body(&body, "gpt-5-thinking", true, None);
        assert_eq!(out["thinking_effort"], json!("max"));
    }

    #[test]
    fn maps_responses_api_effort_aliases() {
        let body = json!({
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "hi"}],
            "extra_body": {"reasoning": {"effort": "high"}}
        });
        let out = build_conversation_body(&body, "gpt-5-4", true, None);
        assert_eq!(out["thinking_effort"], json!("max"));
    }

    #[test]
    fn omits_thinking_effort_when_absent() {
        let body = json!({"messages": [{"role": "user", "content": "hi"}]});
        let out = build_conversation_body(&body, "gpt-5-4", true, None);
        assert!(out.get("thinking_effort").is_none());
    }

    #[test]
    fn forwards_explicit_system_hints() {
        let body = json!({
            "messages": [{"role": "user", "content": "hi"}],
            "system_hints": ["picture_v2", "search"],
        });
        let out = build_conversation_body(&body, "gpt-5-4", true, None);
        assert_eq!(out["system_hints"], json!(["picture_v2", "search"]));
    }

    #[test]
    fn forwards_extra_body_system_hints() {
        let body = json!({
            "messages": [{"role": "user", "content": "hi"}],
            "extra_body": {"system_hints": ["canvas"]},
        });
        let out = build_conversation_body(&body, "gpt-5-4", true, None);
        assert_eq!(out["system_hints"], json!(["canvas"]));
    }

    #[test]
    fn maps_openai_tools_to_system_hints() {
        // Standard OpenAI Responses API `tools` array → chatgpt system_hints.
        for (tool_type, expected) in [
            ("image_generation", "picture_v2"),
            ("web_search_preview", "search"),
            ("web_search", "search"),
            ("deep_research", "connector:connector_openai_deep_research"),
        ] {
            let body = json!({
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"type": tool_type}],
            });
            let out = build_conversation_body(&body, "gpt-5-4", true, None);
            assert_eq!(
                out["system_hints"],
                json!([expected]),
                "tool type {tool_type} should map to hint {expected}"
            );
        }
    }

    #[test]
    fn unrecognized_openai_tool_is_dropped() {
        // User-defined / unknown tool types → no hint (chatgpt-web has no
        // equivalent). Other channels still forward them natively.
        let body = json!({
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function", "function": {"name": "foo"}}],
        });
        let out = build_conversation_body(&body, "gpt-5-4", true, None);
        let empty: Vec<String> = Vec::new();
        assert_eq!(out["system_hints"], json!(empty));
    }

    #[test]
    fn ignores_model_name_suffix() {
        // Body with `@image` suffix used to inject `picture_v2`; the
        // suffix-parser was removed, so no hint is produced now.
        let body = json!({
            "model": "gpt-5@image",
            "messages": [{"role": "user", "content": "hi"}],
        });
        let out = build_conversation_body(&body, "gpt-5@image", true, None);
        let empty: Vec<String> = Vec::new();
        assert_eq!(out["system_hints"], json!(empty));
    }
}
