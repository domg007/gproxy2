//! Content-generation-aware rule applications: prelude system text and claude
//! cache breakpoints. These must know the provider-native body shape.

use serde_json::{Value, json};

use super::compile::CacheBreakpointCfg;
use crate::protocol::ContentGenerationKind;

/// Prepend system text in the target kind's native location.
pub fn prelude_system(body: &mut Value, kind: Option<ContentGenerationKind>, text: &str) {
    use ContentGenerationKind as K;
    let Some(obj) = body.as_object_mut() else {
        return warn_skip("prelude_system", "body not an object");
    };
    match kind {
        Some(K::ClaudeMessages) => match obj.get_mut("system") {
            None | Some(Value::Null) => {
                obj.insert("system".to_owned(), json!(text));
            }
            Some(Value::String(s)) => *s = format!("{text}\n\n{s}"),
            Some(Value::Array(arr)) => arr.insert(0, json!({"type": "text", "text": text})),
            Some(_) => warn_skip("prelude_system", "unexpected claude system shape"),
        },
        Some(K::OpenAiChatCompletions) => match obj.get_mut("messages") {
            Some(Value::Array(msgs)) => {
                msgs.insert(0, json!({"role": "system", "content": text}));
            }
            _ => warn_skip("prelude_system", "missing messages array"),
        },
        Some(K::OpenAiResponses) => match obj.get_mut("instructions") {
            None | Some(Value::Null) => {
                obj.insert("instructions".to_owned(), json!(text));
            }
            Some(Value::String(s)) => *s = format!("{text}\n\n{s}"),
            Some(_) => warn_skip("prelude_system", "unexpected instructions shape"),
        },
        Some(K::GeminiGenerateContent) => {
            let part = json!({"text": text});
            match obj.get_mut("systemInstruction") {
                None | Some(Value::Null) => {
                    obj.insert("systemInstruction".to_owned(), json!({"parts": [part]}));
                }
                Some(Value::Object(si)) => match si.get_mut("parts") {
                    Some(Value::Array(parts)) => parts.insert(0, part),
                    _ => {
                        si.insert("parts".to_owned(), json!([part]));
                    }
                },
                Some(_) => warn_skip("prelude_system", "unexpected systemInstruction shape"),
            }
        }
        None => warn_skip("prelude_system", "non-content operation"),
    }
}

/// Insert a `cache_control` marker (claude wire only; other kinds skip).
pub fn cache_breakpoint(
    body: &mut Value,
    kind: Option<ContentGenerationKind>,
    cfg: &CacheBreakpointCfg,
) {
    if kind != Some(ContentGenerationKind::ClaudeMessages) {
        return warn_skip("cache_breakpoint", "non-claude target");
    }
    let Some(obj) = body.as_object_mut() else {
        return warn_skip("cache_breakpoint", "body not an object");
    };
    let mut control = json!({"type": "ephemeral"});
    if let Some(ttl) = &cfg.ttl {
        control["ttl"] = json!(ttl);
    }
    let blocks: Option<&mut Vec<Value>> = match cfg.target.as_str() {
        // string-form `system` cannot carry block markers — skips via None
        "system" => obj.get_mut("system").and_then(Value::as_array_mut),
        "tools" => obj.get_mut("tools").and_then(Value::as_array_mut),
        "last_message" => obj
            .get_mut("messages")
            .and_then(Value::as_array_mut)
            .and_then(|m| m.last_mut())
            .and_then(|m| m.get_mut("content"))
            .and_then(Value::as_array_mut),
        _ => None,
    };
    let Some(blocks) = blocks else {
        return warn_skip("cache_breakpoint", "target not found or not a block array");
    };
    let idx = cfg
        .index
        .and_then(|i| usize::try_from(i).ok())
        .unwrap_or(blocks.len().saturating_sub(1));
    if let Some(Value::Object(block)) = blocks.get_mut(idx) {
        block.insert("cache_control".to_owned(), control);
    }
}

fn warn_skip(rule: &str, reason: &str) {
    tracing::warn!(rule, reason, "process rule skipped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ContentGenerationKind as K;

    #[test]
    fn prelude_per_kind() {
        let mut claude = json!({"system": "old", "messages": []});
        prelude_system(&mut claude, Some(K::ClaudeMessages), "P");
        assert_eq!(claude["system"], "P\n\nold");

        let mut chat = json!({"messages": [{"role": "user", "content": "hi"}]});
        prelude_system(&mut chat, Some(K::OpenAiChatCompletions), "P");
        assert_eq!(chat["messages"][0]["role"], "system");

        let mut gem = json!({"contents": []});
        prelude_system(&mut gem, Some(K::GeminiGenerateContent), "P");
        assert_eq!(gem["systemInstruction"]["parts"][0]["text"], "P");
    }

    #[test]
    fn cache_breakpoint_last_message() {
        let mut v = json!({"messages": [
            {"role": "user", "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]}
        ]});
        let cfg = CacheBreakpointCfg {
            target: "last_message".into(),
            index: None,
            ttl: Some("5m".into()),
            position: None,
        };
        cache_breakpoint(&mut v, Some(K::ClaudeMessages), &cfg);
        assert_eq!(v["messages"][0]["content"][1]["cache_control"]["ttl"], "5m");
        assert!(
            v["messages"][0]["content"][0]
                .get("cache_control")
                .is_none()
        );
    }
}
