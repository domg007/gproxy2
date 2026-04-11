# Channel-Level Request Body Rewrite Rules — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add channel-level JSON request body rewriting (set/remove at dot-notation paths with optional model/operation/protocol filtering).

**Architecture:** New `utils/rewrite.rs` module with types and pure functions. `ChannelSettings` trait gets a `rewrite_rules()` method. Engine inserts a rewrite step after suffix processing, before `finalize_request`. Frontend adds `rewrite_rules` to common settings fields.

**Tech Stack:** Rust (serde_json for JSON manipulation, hand-rolled glob for model pattern matching), TypeScript (Vue/channel-forms.ts)

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `sdk/gproxy-provider/src/utils/rewrite.rs` | Create | RewriteRule/RewriteAction/RewriteFilter types, `apply_rewrite_rules`, `set_path`, `remove_path`, `matches_filter`, `glob_match`, unit tests |
| `sdk/gproxy-provider/src/utils/mod.rs` | Modify (line 9) | Add `pub mod rewrite;` |
| `sdk/gproxy-provider/src/channel.rs` | Modify (line 189) | Add `rewrite_rules()` default method to `ChannelSettings` trait |
| `sdk/gproxy-provider/src/channels/*.rs` (14 files) | Modify | Add `rewrite_rules` field to Settings struct + impl method |
| `sdk/gproxy-provider/src/store.rs` | Modify (lines 197, 464) | Add `rewrite_rules()` to `ProviderRuntime` trait + impl |
| `sdk/gproxy-provider/src/engine.rs` | Modify (lines 973, 1279) | Insert rewrite step in both `execute_inner` and `execute_stream_inner` |
| `frontend/console/src/modules/admin/providers/channel-forms.ts` | Modify (line 40) | Add `rewrite_rules` to `COMMON_SETTINGS_FIELDS` |

---

### Task 1: Create rewrite module with types and core algorithm

**Files:**
- Create: `sdk/gproxy-provider/src/utils/rewrite.rs`
- Modify: `sdk/gproxy-provider/src/utils/mod.rs`

- [ ] **Step 1: Write failing tests for the rewrite module**

Create `sdk/gproxy-provider/src/utils/rewrite.rs` with types and tests (implementation stubs that panic):

```rust
use gproxy_protocol::kinds::{OperationFamily, ProtocolKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single JSON-path rewrite rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewriteRule {
    /// Dot-separated JSON path, e.g. `"temperature"`, `"metadata.source"`.
    pub path: String,
    /// Set or Remove.
    pub action: RewriteAction,
    /// Optional filter — rule fires only when *all* specified conditions match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<RewriteFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum RewriteAction {
    Set(Value),
    Remove,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RewriteFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<OperationFamily>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocols: Option<Vec<ProtocolKind>>,
}

// ---------------------------------------------------------------------------
// Public API  (stubs — will be implemented in Step 3)
// ---------------------------------------------------------------------------

pub fn apply_rewrite_rules(
    _body: &mut Value,
    _rules: &[RewriteRule],
    _model: Option<&str>,
    _operation: OperationFamily,
    _protocol: ProtocolKind,
) {
    todo!()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn op() -> OperationFamily { OperationFamily::GenerateContent }
    fn proto() -> ProtocolKind { ProtocolKind::OpenAiChatCompletion }

    fn rule(path: &str, action: RewriteAction) -> RewriteRule {
        RewriteRule { path: path.to_string(), action, filter: None }
    }

    fn filtered_rule(path: &str, action: RewriteAction, filter: RewriteFilter) -> RewriteRule {
        RewriteRule { path: path.to_string(), action, filter: Some(filter) }
    }

    // --- Set ---

    #[test]
    fn set_scalar_top_level() {
        let mut body = json!({"temperature": 1.0});
        apply_rewrite_rules(&mut body, &[rule("temperature", RewriteAction::Set(json!(0.7)))], Some("gpt-4o"), op(), proto());
        assert_eq!(body["temperature"], json!(0.7));
    }

    #[test]
    fn set_nested_parents_exist() {
        let mut body = json!({"a": {"b": {}}});
        apply_rewrite_rules(&mut body, &[rule("a.b.c", RewriteAction::Set(json!("hello")))], None, op(), proto());
        assert_eq!(body["a"]["b"]["c"], json!("hello"));
    }

    #[test]
    fn set_nested_parents_missing() {
        let mut body = json!({});
        apply_rewrite_rules(&mut body, &[rule("a.b.c", RewriteAction::Set(json!(true)))], None, op(), proto());
        assert_eq!(body, json!({"a": {"b": {"c": true}}}));
    }

    #[test]
    fn set_object_value() {
        let mut body = json!({});
        apply_rewrite_rules(&mut body, &[rule("metadata", RewriteAction::Set(json!({"source": "gproxy"})))], None, op(), proto());
        assert_eq!(body["metadata"], json!({"source": "gproxy"}));
    }

    #[test]
    fn set_array_value() {
        let mut body = json!({});
        apply_rewrite_rules(&mut body, &[rule("stop", RewriteAction::Set(json!(["END", "STOP"])))], None, op(), proto());
        assert_eq!(body["stop"], json!(["END", "STOP"]));
    }

    #[test]
    fn set_null_value() {
        let mut body = json!({"user": "bob"});
        apply_rewrite_rules(&mut body, &[rule("user", RewriteAction::Set(json!(null)))], None, op(), proto());
        assert_eq!(body["user"], json!(null));
    }

    #[test]
    fn set_overwrites_non_object_intermediate() {
        let mut body = json!({"a": "string_value"});
        apply_rewrite_rules(&mut body, &[rule("a.b.c", RewriteAction::Set(json!(42)))], None, op(), proto());
        assert_eq!(body, json!({"a": {"b": {"c": 42}}}));
    }

    // --- Remove ---

    #[test]
    fn remove_existing_top_level() {
        let mut body = json!({"temperature": 1.0, "model": "x"});
        apply_rewrite_rules(&mut body, &[rule("temperature", RewriteAction::Remove)], None, op(), proto());
        assert_eq!(body, json!({"model": "x"}));
    }

    #[test]
    fn remove_nested() {
        let mut body = json!({"a": {"b": {"c": 1, "d": 2}}});
        apply_rewrite_rules(&mut body, &[rule("a.b.c", RewriteAction::Remove)], None, op(), proto());
        assert_eq!(body, json!({"a": {"b": {"d": 2}}}));
    }

    #[test]
    fn remove_nonexistent_silent() {
        let mut body = json!({"a": 1});
        let original = body.clone();
        apply_rewrite_rules(&mut body, &[rule("x.y.z", RewriteAction::Remove)], None, op(), proto());
        assert_eq!(body, original);
    }

    // --- Filters ---

    #[test]
    fn filter_model_pattern_match() {
        let mut body = json!({"temperature": 1.0});
        let filter = RewriteFilter { model_pattern: Some("gpt-4*".into()), ..Default::default() };
        apply_rewrite_rules(&mut body, &[filtered_rule("temperature", RewriteAction::Set(json!(0.5)), filter)], Some("gpt-4o"), op(), proto());
        assert_eq!(body["temperature"], json!(0.5));
    }

    #[test]
    fn filter_model_pattern_no_match() {
        let mut body = json!({"temperature": 1.0});
        let filter = RewriteFilter { model_pattern: Some("gpt-4*".into()), ..Default::default() };
        apply_rewrite_rules(&mut body, &[filtered_rule("temperature", RewriteAction::Set(json!(0.5)), filter)], Some("claude-3-opus"), op(), proto());
        assert_eq!(body["temperature"], json!(1.0));
    }

    #[test]
    fn filter_operation_match() {
        let mut body = json!({"temperature": 1.0});
        let filter = RewriteFilter { operations: Some(vec![OperationFamily::GenerateContent]), ..Default::default() };
        apply_rewrite_rules(&mut body, &[filtered_rule("temperature", RewriteAction::Set(json!(0.5)), filter)], None, OperationFamily::GenerateContent, proto());
        assert_eq!(body["temperature"], json!(0.5));
    }

    #[test]
    fn filter_operation_no_match() {
        let mut body = json!({"temperature": 1.0});
        let filter = RewriteFilter { operations: Some(vec![OperationFamily::GenerateContent]), ..Default::default() };
        apply_rewrite_rules(&mut body, &[filtered_rule("temperature", RewriteAction::Set(json!(0.5)), filter)], None, OperationFamily::ModelList, proto());
        assert_eq!(body["temperature"], json!(1.0));
    }

    #[test]
    fn filter_protocol_match() {
        let mut body = json!({"temperature": 1.0});
        let filter = RewriteFilter { protocols: Some(vec![ProtocolKind::OpenAiChatCompletion]), ..Default::default() };
        apply_rewrite_rules(&mut body, &[filtered_rule("temperature", RewriteAction::Set(json!(0.5)), filter)], None, op(), ProtocolKind::OpenAiChatCompletion);
        assert_eq!(body["temperature"], json!(0.5));
    }

    #[test]
    fn filter_and_logic_all_must_match() {
        let mut body = json!({"temperature": 1.0});
        let filter = RewriteFilter {
            model_pattern: Some("gpt-4*".into()),
            operations: Some(vec![OperationFamily::GenerateContent]),
            protocols: None,
        };
        // model matches but operation doesn't
        apply_rewrite_rules(&mut body, &[filtered_rule("temperature", RewriteAction::Set(json!(0.5)), filter)], Some("gpt-4o"), OperationFamily::ModelList, proto());
        assert_eq!(body["temperature"], json!(1.0));
    }

    // --- Multiple rules ---

    #[test]
    fn multiple_rules_sequential() {
        let mut body = json!({"temperature": 1.0, "top_p": 0.9});
        let rules = vec![
            rule("temperature", RewriteAction::Set(json!(0.7))),
            rule("top_p", RewriteAction::Remove),
        ];
        apply_rewrite_rules(&mut body, &rules, None, op(), proto());
        assert_eq!(body, json!({"temperature": 0.7}));
    }

    #[test]
    fn later_rule_overwrites_earlier() {
        let mut body = json!({});
        let rules = vec![
            rule("temperature", RewriteAction::Set(json!(0.5))),
            rule("temperature", RewriteAction::Set(json!(0.9))),
        ];
        apply_rewrite_rules(&mut body, &rules, None, op(), proto());
        assert_eq!(body["temperature"], json!(0.9));
    }

    // --- Edge cases ---

    #[test]
    fn non_object_body_skipped() {
        let mut body = json!([1, 2, 3]);
        let original = body.clone();
        apply_rewrite_rules(&mut body, &[rule("temperature", RewriteAction::Set(json!(0.7)))], None, op(), proto());
        assert_eq!(body, original);
    }

    #[test]
    fn empty_rules_noop() {
        let mut body = json!({"temperature": 1.0});
        let original = body.clone();
        apply_rewrite_rules(&mut body, &[], None, op(), proto());
        assert_eq!(body, original);
    }

    // --- Glob matching ---

    #[test]
    fn glob_star_suffix() {
        assert!(super::glob_match("gpt-4*", "gpt-4o"));
        assert!(super::glob_match("gpt-4*", "gpt-4"));
        assert!(!super::glob_match("gpt-4*", "gpt-3.5-turbo"));
    }

    #[test]
    fn glob_star_prefix() {
        assert!(super::glob_match("*-turbo", "gpt-3.5-turbo"));
        assert!(!super::glob_match("*-turbo", "gpt-4o"));
    }

    #[test]
    fn glob_star_middle() {
        assert!(super::glob_match("claude-*-opus", "claude-3-opus"));
        assert!(!super::glob_match("claude-*-opus", "claude-3-sonnet"));
    }

    #[test]
    fn glob_question_mark() {
        assert!(super::glob_match("gpt-?o", "gpt-4o"));
        assert!(!super::glob_match("gpt-?o", "gpt-4op"));
    }

    #[test]
    fn glob_exact() {
        assert!(super::glob_match("gpt-4o", "gpt-4o"));
        assert!(!super::glob_match("gpt-4o", "gpt-4o-mini"));
    }

    // --- Serde round-trip ---

    #[test]
    fn serde_roundtrip_set() {
        let rule = RewriteRule {
            path: "temperature".into(),
            action: RewriteAction::Set(json!(0.7)),
            filter: None,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: RewriteRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.path, "temperature");
        assert!(matches!(back.action, RewriteAction::Set(v) if v == json!(0.7)));
    }

    #[test]
    fn serde_roundtrip_remove() {
        let rule = RewriteRule {
            path: "stream_options".into(),
            action: RewriteAction::Remove,
            filter: Some(RewriteFilter {
                model_pattern: Some("gpt-4*".into()),
                operations: None,
                protocols: None,
            }),
        };
        let json = serde_json::to_string(&rule).unwrap();
        let back: RewriteRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.path, "stream_options");
        assert!(matches!(back.action, RewriteAction::Remove));
        assert_eq!(back.filter.unwrap().model_pattern, Some("gpt-4*".into()));
    }
}
```

- [ ] **Step 2: Register the module**

In `sdk/gproxy-provider/src/utils/mod.rs`, add after line 9 (`pub mod sanitize;`):

```rust
pub mod rewrite;
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p gproxy-provider utils::rewrite::tests -- --nocapture 2>&1 | head -30`
Expected: compile succeeds, all tests FAIL with `not yet implemented`

- [ ] **Step 4: Implement the core functions**

Replace the `apply_rewrite_rules` stub and add private helpers in `sdk/gproxy-provider/src/utils/rewrite.rs`:

```rust
// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn apply_rewrite_rules(
    body: &mut Value,
    rules: &[RewriteRule],
    model: Option<&str>,
    operation: OperationFamily,
    protocol: ProtocolKind,
) {
    if rules.is_empty() {
        return;
    }
    let Some(_) = body.as_object() else {
        return;
    };
    for rule in rules {
        if !matches_filter(&rule.filter, model, operation, protocol) {
            continue;
        }
        let segments: Vec<&str> = rule.path.split('.').collect();
        if segments.is_empty() || segments.iter().any(|s| s.is_empty()) {
            continue;
        }
        match &rule.action {
            RewriteAction::Set(value) => set_path(body, &segments, value.clone()),
            RewriteAction::Remove => remove_path(body, &segments),
        }
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn set_path(body: &mut Value, segments: &[&str], value: Value) {
    let mut current = body;
    for &segment in &segments[..segments.len() - 1] {
        if !current.is_object() {
            *current = Value::Object(serde_json::Map::new());
        }
        let map = current.as_object_mut().unwrap();
        if !map.contains_key(segment) || !map[segment].is_object() {
            map.insert(segment.to_string(), Value::Object(serde_json::Map::new()));
        }
        current = map.get_mut(segment).unwrap();
    }
    if !current.is_object() {
        *current = Value::Object(serde_json::Map::new());
    }
    current
        .as_object_mut()
        .unwrap()
        .insert(segments[segments.len() - 1].to_string(), value);
}

fn remove_path(body: &mut Value, segments: &[&str]) {
    let mut current = body;
    for &segment in &segments[..segments.len() - 1] {
        match current.as_object_mut().and_then(|m| m.get_mut(segment)) {
            Some(next) => current = next,
            None => return,
        }
    }
    if let Some(map) = current.as_object_mut() {
        map.remove(segments[segments.len() - 1]);
    }
}

// ---------------------------------------------------------------------------
// Filter matching
// ---------------------------------------------------------------------------

fn matches_filter(
    filter: &Option<RewriteFilter>,
    model: Option<&str>,
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> bool {
    let Some(f) = filter else {
        return true;
    };
    if let Some(ref pattern) = f.model_pattern {
        match model {
            Some(m) if glob_match(pattern, m) => {}
            _ => return false,
        }
    }
    if let Some(ref ops) = f.operations {
        if !ops.contains(&operation) {
            return false;
        }
    }
    if let Some(ref protos) = f.protocols {
        if !protos.contains(&protocol) {
            return false;
        }
    }
    true
}

/// Minimal glob matcher supporting `*` (any chars) and `?` (one char).
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pi, mut star_ti) = (usize::MAX, 0usize);

    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p gproxy-provider utils::rewrite::tests -- --nocapture`
Expected: all 24 tests PASS

- [ ] **Step 6: Commit**

```bash
git add sdk/gproxy-provider/src/utils/rewrite.rs sdk/gproxy-provider/src/utils/mod.rs
git commit -m "feat: add rewrite module with types, path helpers, glob matcher, and tests"
```

---

### Task 2: Extend ChannelSettings trait with rewrite_rules()

**Files:**
- Modify: `sdk/gproxy-provider/src/channel.rs:189`

- [ ] **Step 1: Add rewrite_rules() to ChannelSettings trait**

In `sdk/gproxy-provider/src/channel.rs`, after the `enable_suffix()` method (line 188), add:

```rust
    /// JSON-path rewrite rules applied to the request body before
    /// `finalize_request`. Rules are executed in declaration order.
    fn rewrite_rules(&self) -> &[crate::utils::rewrite::RewriteRule] {
        &[]
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p gproxy-provider`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add sdk/gproxy-provider/src/channel.rs
git commit -m "feat: add rewrite_rules() to ChannelSettings trait"
```

---

### Task 3: Add rewrite_rules field to all 14 channel Settings structs

**Files:**
- Modify: `sdk/gproxy-provider/src/channels/openai.rs` (struct line 25, impl line 51)
- Modify: `sdk/gproxy-provider/src/channels/anthropic.rs` (struct line 46, impl line 72)
- Modify: `sdk/gproxy-provider/src/channels/aistudio.rs` (struct line 26, impl line 52)
- Modify: `sdk/gproxy-provider/src/channels/vertex.rs` (struct line 45, impl line 68)
- Modify: `sdk/gproxy-provider/src/channels/vertexexpress.rs` (struct line 26, impl line 52)
- Modify: `sdk/gproxy-provider/src/channels/geminicli.rs` (struct line 308, impl line 367)
- Modify: `sdk/gproxy-provider/src/channels/antigravity.rs` (struct line 275, impl line 312)
- Modify: `sdk/gproxy-provider/src/channels/claudecode.rs` (struct line 283, impl line 298)
- Modify: `sdk/gproxy-provider/src/channels/codex.rs` (struct line 199, impl line 449)
- Modify: `sdk/gproxy-provider/src/channels/nvidia.rs` (struct line 26, impl line 45)
- Modify: `sdk/gproxy-provider/src/channels/deepseek.rs` (struct line 38, impl line 64)
- Modify: `sdk/gproxy-provider/src/channels/groq.rs` (struct line 26, impl line 51)
- Modify: `sdk/gproxy-provider/src/channels/openrouter.rs` (struct line 26, impl line 52)
- Modify: `sdk/gproxy-provider/src/channels/custom.rs` (struct line 27, impl line 46)

For each of the 14 channel files, apply the same two edits:

- [ ] **Step 1: Add field to each Settings struct**

In every channel's Settings struct, after the `sanitize_rules` field and before `enable_suffix`, add:

```rust
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rewrite_rules: Vec<crate::utils::rewrite::RewriteRule>,
```

Example for `openai.rs` — the struct becomes:
```rust
pub struct OpenAiSettings {
    #[serde(default = "default_openai_base_url")]
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries_on_429: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sanitize_rules: Vec<crate::utils::sanitize::SanitizeRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rewrite_rules: Vec<crate::utils::rewrite::RewriteRule>,
    #[serde(default)]
    pub enable_suffix: bool,
}
```

- [ ] **Step 2: Add impl method to each ChannelSettings impl**

In each channel's `impl ChannelSettings for *Settings`, after `sanitize_rules()` and before `enable_suffix()`, add:

```rust
    fn rewrite_rules(&self) -> &[crate::utils::rewrite::RewriteRule] {
        &self.rewrite_rules
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p gproxy-provider`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add sdk/gproxy-provider/src/channels/
git commit -m "feat: add rewrite_rules field to all 14 channel Settings structs"
```

---

### Task 4: Expose rewrite_rules through ProviderRuntime

**Files:**
- Modify: `sdk/gproxy-provider/src/store.rs:197,464`

- [ ] **Step 1: Add to ProviderRuntime trait**

In `sdk/gproxy-provider/src/store.rs`, after the `sanitize_rules()` method (line 197), add:

```rust
    fn rewrite_rules(&self) -> Vec<crate::utils::rewrite::RewriteRule>;
```

- [ ] **Step 2: Add to the blanket impl**

In the same file, after the `sanitize_rules()` impl (around line 466), add:

```rust
    fn rewrite_rules(&self) -> Vec<crate::utils::rewrite::RewriteRule> {
        self.settings.load().rewrite_rules().to_vec()
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p gproxy-provider`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add sdk/gproxy-provider/src/store.rs
git commit -m "feat: expose rewrite_rules through ProviderRuntime"
```

---

### Task 5: Integrate rewrite into engine pipeline

**Files:**
- Modify: `sdk/gproxy-provider/src/engine.rs:973,1279`

- [ ] **Step 1: Add rewrite step in execute_inner**

In `sdk/gproxy-provider/src/engine.rs`, in `execute_inner`, **before** the line `let mut prepared = provider.finalize_request(prepared)?;` (line 973), insert:

```rust
        // Rewrite rules: apply JSON path set/remove before channel-specific
        // finalize so finalize can process the rewritten values normally.
        let rewrite_rules = provider.rewrite_rules();
        if !rewrite_rules.is_empty() {
            if let Ok(mut body_json) = serde_json::from_slice::<Value>(&prepared.body) {
                crate::utils::rewrite::apply_rewrite_rules(
                    &mut body_json,
                    &rewrite_rules,
                    prepared.model.as_deref(),
                    prepared.route.operation,
                    prepared.route.protocol,
                );
                if let Ok(patched) = serde_json::to_vec(&body_json) {
                    prepared.body = patched;
                }
            }
        }
```

- [ ] **Step 2: Add rewrite step in execute_stream_inner**

In the same file, in `execute_stream_inner`, **before** the line `let mut prepared = provider.finalize_request(prepared)?;` (line 1279), insert the exact same block:

```rust
        // Rewrite rules: apply JSON path set/remove before channel-specific
        // finalize so finalize can process the rewritten values normally.
        let rewrite_rules = provider.rewrite_rules();
        if !rewrite_rules.is_empty() {
            if let Ok(mut body_json) = serde_json::from_slice::<Value>(&prepared.body) {
                crate::utils::rewrite::apply_rewrite_rules(
                    &mut body_json,
                    &rewrite_rules,
                    prepared.model.as_deref(),
                    prepared.route.operation,
                    prepared.route.protocol,
                );
                if let Ok(patched) = serde_json::to_vec(&body_json) {
                    prepared.body = patched;
                }
            }
        }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p gproxy-provider`
Expected: compiles with no errors

- [ ] **Step 4: Run all existing tests to verify no regressions**

Run: `cargo test -p gproxy-provider`
Expected: all tests PASS (rewrite is a no-op when rules are empty, so existing behavior is unchanged)

- [ ] **Step 5: Commit**

```bash
git add sdk/gproxy-provider/src/engine.rs
git commit -m "feat: integrate rewrite rules into engine pipeline before finalize_request"
```

---

### Task 6: Add rewrite_rules to frontend common settings

**Files:**
- Modify: `frontend/console/src/modules/admin/providers/channel-forms.ts:40`

- [ ] **Step 1: Add to COMMON_SETTINGS_FIELDS**

In `frontend/console/src/modules/admin/providers/channel-forms.ts`, in the `COMMON_SETTINGS_FIELDS` array (line 38-41), add the `rewrite_rules` field after `sanitize_rules`:

```typescript
const COMMON_SETTINGS_FIELDS: ChannelField[] = [
  { key: "enable_suffix", label: "enable_suffix", type: "boolean", optional: true },
  { key: "sanitize_rules", label: "sanitize_rules", type: "json", optional: true },
  { key: "rewrite_rules", label: "rewrite_rules", type: "json", optional: true },
];
```

- [ ] **Step 2: Verify frontend builds**

Run: `cd frontend/console && npm run build`
Expected: build succeeds

- [ ] **Step 3: Commit**

```bash
git add frontend/console/src/modules/admin/providers/channel-forms.ts
git commit -m "feat: add rewrite_rules to frontend common settings fields"
```

---

### Task 7: Full build verification

- [ ] **Step 1: Run full cargo build**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: all tests PASS

- [ ] **Step 3: Run frontend build**

Run: `cd frontend/console && npm run build`
Expected: build succeeds

- [ ] **Step 4: Final commit (if any unstaged changes)**

Only if there are leftover fixes discovered during full build.
