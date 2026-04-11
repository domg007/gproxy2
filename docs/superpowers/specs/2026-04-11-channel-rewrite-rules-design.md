# Channel-Level Request Body Rewrite Rules

## Overview

Add a channel-level parameter rewriting feature to gproxy that allows modifying the JSON request body before it is sent upstream. Each provider can configure a set of rewrite rules that **set** or **remove** values at arbitrary JSON paths (dot-notation, e.g. `a.b.c.d`).

## Goals

- Allow operators to force, override, or strip request body parameters per channel.
- Support all 6 JSON value types for overwrite targets: string, number, boolean, null, object, array.
- Provide optional filtering by model pattern (glob), operation family, and protocol kind.
- Integrate as a clean, independent step in the engine pipeline.

## Non-Goals

- No response body rewriting (request-only).
- No JSONPath query syntax (only simple dot-separated key paths).
- No conditional logic beyond filter matching (no "if field X equals Y then set Z").

---

## Data Model

### RewriteRule

Stored in provider `settings_json` under a `rewrite_rules` array.

```rust
/// A single JSON path rewrite rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewriteRule {
    /// JSON path using dot notation, e.g. "temperature", "metadata.source".
    pub path: String,
    /// The rewrite action.
    pub action: RewriteAction,
    /// Optional filter — rule only applies when ALL specified conditions match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<RewriteFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum RewriteAction {
    /// Set (or create) the value at the given path.
    Set(serde_json::Value),
    /// Remove the value at the given path.
    Remove,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RewriteFilter {
    /// Glob pattern matched against the model name, e.g. "gpt-4*", "claude-*".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_pattern: Option<String>,
    /// Restrict to these operation families.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<OperationFamily>>,
    /// Restrict to these protocol kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocols: Option<Vec<ProtocolKind>>,
}
```

### Configuration Example

```json
{
  "rewrite_rules": [
    {
      "path": "temperature",
      "action": { "type": "Set", "value": 0.7 }
    },
    {
      "path": "metadata.source",
      "action": { "type": "Set", "value": "gproxy" }
    },
    {
      "path": "stream_options",
      "action": { "type": "Remove" },
      "filter": { "model_pattern": "gpt-4*" }
    },
    {
      "path": "generationConfig.topK",
      "action": { "type": "Set", "value": 40 },
      "filter": {
        "protocols": ["Gemini"],
        "operations": ["GenerateContent", "StreamGenerateContent"]
      }
    }
  ]
}
```

---

## Core Algorithm

File: `sdk/gproxy-provider/src/utils/rewrite.rs`

### apply_rewrite_rules

```rust
pub fn apply_rewrite_rules(
    body: &mut Value,
    rules: &[RewriteRule],
    model: Option<&str>,
    operation: OperationFamily,
    protocol: ProtocolKind,
) {
    for rule in rules {
        if !matches_filter(&rule.filter, model, operation, protocol) {
            continue;
        }
        let segments: Vec<&str> = rule.path.split('.').collect();
        match &rule.action {
            RewriteAction::Set(value) => set_path(body, &segments, value.clone()),
            RewriteAction::Remove => remove_path(body, &segments),
        }
    }
}
```

### Path Operations

- **`set_path(body, segments, value)`**: Walk the path segment by segment. For each intermediate segment, if the key doesn't exist, create a new empty object (`{}`). If the key exists but is not an object (e.g. a string or number), overwrite it with an empty object to continue traversal. At the final segment, insert or overwrite the value.
- **`remove_path(body, segments)`**: Walk to the second-to-last segment. If any intermediate segment is missing or not an object, return silently. At the final segment, call `map.remove(key)`.
- **`matches_filter(filter, model, operation, protocol)`**: All specified dimensions are AND-ed. `model_pattern` uses glob matching. `operations`/`protocols` check `Vec::contains`. A `None` filter or all-`None` fields means unconditional match.

### Error Handling

- Path not found on Remove → silent skip.
- Path not found on Set → auto-create parent objects.
- Body is not a JSON object → skip all rules silently.
- Rules are executed sequentially in declaration order; later rules can overwrite earlier results.

---

## Pipeline Integration

### Position in Engine

In `engine.rs`, **after suffix processing and PreparedRequest construction, before `finalize_request`**:

```
pipeline:
  → dispatch table lookup (dst_op, dst_proto)
  → protocol transform (if needed)
  → inject_stream_flag
  → suffix processing
  → PreparedRequest construction + suffix apply_fns
  → **apply_rewrite_rules**          ← HERE
  → finalize_request (channel-specific normalization)
  → sanitize_rules (regex text replacement)
  → prepare_request (auth headers, HTTP wrapping)
  → send upstream
```

**Rationale**: Rewrite operates on the pre-normalization body so that channel-specific finalize logic (e.g. Anthropic's magic cache, beta headers, sampling param stripping) can still process the rewritten values correctly. Suffix has higher priority than rewrite (suffix runs first).

### Integration in Both Paths

Apply in both `execute_inner` (non-streaming) and `execute_stream_inner` (streaming) at the equivalent position.

### Reading Rules

Add a `rewrite_rules()` method to the `ChannelSettings` trait (default: empty slice), mirroring the existing `sanitize_rules()` pattern. Each channel's Settings struct gains a `#[serde(default)] rewrite_rules: Vec<RewriteRule>` field.

The engine calls `provider.rewrite_rules()` and, if non-empty, deserializes `prepared.body` into `serde_json::Value`, applies rules, then serializes back.

```rust
// In engine.rs, before finalize_request:
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

---

## Frontend UI

### Settings Field

Add `rewrite_rules` to the common settings fields in `channel-forms.ts` for all channels. This is a structured list editor similar to `sanitize_rules`.

### Rule Editor UI

Each rule in the list shows:

| Field | Widget | Notes |
|-------|--------|-------|
| `path` | Text input | Dot-notation path, e.g. `temperature`, `metadata.source` |
| `action.type` | Select dropdown | `Set` or `Remove` |
| `action.value` | JSON input | Only shown when action is `Set`. Accepts raw JSON (string, number, boolean, null, object, array). |
| `filter` | Collapsible section | Optional, hidden by default |
| `filter.model_pattern` | Text input | Glob pattern, e.g. `gpt-4*` |
| `filter.operations` | Multi-select | OperationFamily variants |
| `filter.protocols` | Multi-select | ProtocolKind variants |

---

## Testing

### Unit Tests (in `utils/rewrite.rs`)

1. **Set scalar at top level**: `set "temperature" → 0.7` on `{"temperature": 1.0}` → value is overwritten.
2. **Set nested path, parents exist**: `set "a.b.c" → "hello"` on `{"a": {"b": {}}}` → creates `c`.
3. **Set nested path, parents missing**: `set "a.b.c" → true` on `{}` → creates `{"a": {"b": {"c": true}}}`.
4. **Set with object value**: `set "metadata" → {"source": "gproxy"}` on `{}` → sets entire object.
5. **Set with array value**: `set "stop" → ["END", "STOP"]` on `{}` → sets array.
6. **Set with null value**: `set "user" → null` on `{"user": "bob"}` → sets null.
7. **Remove existing path**: `remove "temperature"` on `{"temperature": 1.0, "model": "x"}` → `{"model": "x"}`.
8. **Remove nested path**: `remove "a.b.c"` on `{"a": {"b": {"c": 1, "d": 2}}}` → `{"a": {"b": {"d": 2}}}`.
9. **Remove non-existent path**: `remove "x.y.z"` on `{"a": 1}` → body unchanged.
10. **Filter: model pattern match**: rule with `model_pattern: "gpt-4*"` applies to model `"gpt-4o"`.
11. **Filter: model pattern no match**: same rule skipped for model `"claude-3-opus"`.
12. **Filter: operation match**: rule with `operations: [GenerateContent]` applies to GenerateContent.
13. **Filter: protocol match**: rule with `protocols: [OpenAi]` applies to OpenAi protocol.
14. **Filter: AND logic**: rule with both model_pattern and operations — both must match.
15. **Multiple rules sequential**: two rules applied in order, second can overwrite first.
16. **Non-object body**: rules silently skipped when body is an array or scalar.

---

## Files to Create/Modify

| File | Action |
|------|--------|
| `sdk/gproxy-provider/src/utils/rewrite.rs` | **Create** — RewriteRule types + apply_rewrite_rules + path helpers + tests |
| `sdk/gproxy-provider/src/utils/mod.rs` | **Modify** — add `pub mod rewrite;` |
| `sdk/gproxy-provider/src/channel.rs` | **Modify** — add `rewrite_rules()` to ChannelSettings trait |
| `sdk/gproxy-provider/src/engine.rs` | **Modify** — insert rewrite step before finalize_request in both execute paths |
| `sdk/gproxy-provider/src/store.rs` | **Modify** — expose rewrite_rules through ProviderRuntime (if needed) |
| `sdk/gproxy-provider/src/channels/*.rs` | **Modify** — add `rewrite_rules` field to each channel's Settings struct |
| `frontend/console/src/modules/admin/providers/channel-forms.ts` | **Modify** — add rewrite_rules to common settings fields |
| Frontend rewrite rule editor component | **Create** — structured list editor for rewrite rules |
