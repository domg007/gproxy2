use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CacheBreakpointTarget {
    #[default]
    TopLevel,
    Tools,
    System,
    Messages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CacheBreakpointPositionKind {
    #[default]
    Nth,
    LastNth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CacheBreakpointTtl {
    #[default]
    Auto,
    Ttl5m,
    Ttl1h,
}

impl CacheBreakpointTtl {
    pub fn ttl(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Ttl5m => Some("5m"),
            Self::Ttl1h => Some("1h"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheBreakpointRule {
    pub target: CacheBreakpointTarget,
    #[serde(default)]
    pub position: CacheBreakpointPositionKind,
    #[serde(default = "default_cache_breakpoint_index")]
    pub index: usize,
    #[serde(default)]
    pub ttl: CacheBreakpointTtl,
}

impl CacheBreakpointRule {
    fn normalized(mut self) -> Self {
        if self.index == 0 {
            self.index = 1;
        }
        self
    }
}

fn default_cache_breakpoint_index() -> usize {
    1
}

const MAGIC_TRIGGER_AUTO_ID: &str =
    "GPROXY_MAGIC_STRING_TRIGGER_CACHING_CREATE_7D9ASD7A98SD7A9S8D79ASC98A7FNKJBVV80SCMSHDSIUCH";
const MAGIC_TRIGGER_5M_ID: &str =
    "GPROXY_MAGIC_STRING_TRIGGER_CACHING_CREATE_49VA1S5V19GR4G89W2V695G9W9GV52W95V198WV5W2FC9DF";
const MAGIC_TRIGGER_1H_ID: &str =
    "GPROXY_MAGIC_STRING_TRIGGER_CACHING_CREATE_1FAS5GV9R5H29T5Y2J9584K6O95M2NBVW52C95CX984FRJY";

pub fn apply_magic_string_cache_control_triggers(body: &mut Value) {
    let Some(root) = body.as_object_mut() else {
        return;
    };

    if let Some(system) = root.get_mut("system") {
        apply_magic_trigger_to_content(system);
    }

    if let Some(messages) = root.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            let Some(message_map) = message.as_object_mut() else {
                continue;
            };
            let Some(content) = message_map.get_mut("content") else {
                continue;
            };
            apply_magic_trigger_to_content(content);
        }
    }
}

fn apply_magic_trigger_to_content(content: &mut Value) {
    match content {
        Value::Array(blocks) => {
            for block in blocks {
                let Some(block_map) = block.as_object_mut() else {
                    continue;
                };
                apply_magic_trigger_to_block(block_map);
            }
        }
        Value::Object(block_map) => apply_magic_trigger_to_block(block_map),
        _ => {}
    }
}

fn apply_magic_trigger_to_block(block_map: &mut serde_json::Map<String, Value>) {
    let Some(Value::String(text)) = block_map.get_mut("text") else {
        return;
    };

    let ttl = remove_magic_trigger_tokens(text);
    let Some(ttl) = ttl else {
        return;
    };

    if !block_map.contains_key("cache_control") {
        block_map.insert("cache_control".to_string(), cache_control_ephemeral(ttl));
    }
}

fn remove_magic_trigger_tokens(text: &mut String) -> Option<CacheBreakpointTtl> {
    let specs = [
        (MAGIC_TRIGGER_AUTO_ID, "auto", CacheBreakpointTtl::Auto),
        (MAGIC_TRIGGER_5M_ID, "5m", CacheBreakpointTtl::Ttl5m),
        (MAGIC_TRIGGER_1H_ID, "1h", CacheBreakpointTtl::Ttl1h),
    ];

    let mut matched_ttl = None;
    for (id, ttl_suffix, ttl) in specs {
        let with_suffix = format!("{id} {ttl_suffix}");
        if text.contains(&with_suffix) {
            *text = text.replace(&with_suffix, "");
            if matched_ttl.is_none() {
                matched_ttl = Some(ttl);
            }
        }
        if text.contains(id) {
            *text = text.replace(id, "");
        }
    }

    matched_ttl
}

pub fn parse_cache_breakpoint_rules(value: Option<&Value>) -> Vec<CacheBreakpointRule> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(parse_cache_breakpoint_rule)
        .take(4)
        .collect()
}

fn parse_cache_breakpoint_rule(item: &Value) -> Option<CacheBreakpointRule> {
    let obj = item.as_object()?;
    let target = match obj.get("target").and_then(Value::as_str)?.trim().to_ascii_lowercase().as_str() {
        "global" | "top_level" => CacheBreakpointTarget::TopLevel,
        "tools" => CacheBreakpointTarget::Tools,
        "system" => CacheBreakpointTarget::System,
        "messages" => CacheBreakpointTarget::Messages,
        _ => return None,
    };

    let position = match obj
        .get("position")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("nth")
        .to_ascii_lowercase()
        .as_str()
    {
        "last" | "last_nth" | "from_end" => CacheBreakpointPositionKind::LastNth,
        _ => CacheBreakpointPositionKind::Nth,
    };

    let index = obj
        .get("index")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(1);

    let ttl = match obj
        .get("ttl")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("auto")
        .to_ascii_lowercase()
        .as_str()
    {
        "5m" | "ttl5m" => CacheBreakpointTtl::Ttl5m,
        "1h" | "ttl1h" => CacheBreakpointTtl::Ttl1h,
        _ => CacheBreakpointTtl::Auto,
    };

    Some(
        CacheBreakpointRule {
            target,
            position,
            index,
            ttl,
        }
        .normalized(),
    )
}

pub fn cache_breakpoint_rules_to_settings_value(rules: &[CacheBreakpointRule]) -> Option<Value> {
    let normalized: Vec<CacheBreakpointRule> = rules
        .iter()
        .cloned()
        .map(CacheBreakpointRule::normalized)
        .take(4)
        .collect();
    if normalized.is_empty() {
        return None;
    }
    serde_json::to_value(normalized).ok()
}

pub fn ensure_cache_breakpoint_rules(body: &mut Value, rules: &[CacheBreakpointRule]) {
    if rules.is_empty() {
        return;
    }
    let Some(root) = body.as_object_mut() else {
        return;
    };
    let existing_breakpoints = existing_cache_breakpoint_count(root);
    let mut remaining_slots = 4usize.saturating_sub(existing_breakpoints);
    if remaining_slots == 0 {
        return;
    }

    for rule in rules.iter().take(4) {
        if remaining_slots == 0 {
            break;
        }
        apply_cache_breakpoint_rule(root, &rule.clone().normalized(), &mut remaining_slots);
    }
}

fn apply_cache_breakpoint_rule(
    root: &mut serde_json::Map<String, Value>,
    rule: &CacheBreakpointRule,
    remaining_slots: &mut usize,
) {
    if *remaining_slots == 0 {
        return;
    }

    match rule.target {
        CacheBreakpointTarget::TopLevel => {
            if !root.contains_key("cache_control") {
                root.insert(
                    "cache_control".to_string(),
                    cache_control_ephemeral(rule.ttl),
                );
                *remaining_slots = remaining_slots.saturating_sub(1);
            }
        }
        CacheBreakpointTarget::Tools => {
            let Some(tools) = root.get_mut("tools").and_then(Value::as_array_mut) else {
                return;
            };
            let Some(idx) = resolve_rule_index(tools.len(), rule.position, rule.index) else {
                return;
            };
            let Some(map) = tools[idx].as_object_mut() else {
                return;
            };
            if !map.contains_key("cache_control") {
                map.insert("cache_control".to_string(), cache_control_ephemeral(rule.ttl));
                *remaining_slots = remaining_slots.saturating_sub(1);
            }
        }
        CacheBreakpointTarget::System => match root.get_mut("system") {
            Some(Value::Array(blocks)) => {
                let Some(idx) = resolve_rule_index(blocks.len(), rule.position, rule.index) else {
                    return;
                };
                let Some(map) = blocks[idx].as_object_mut() else {
                    return;
                };
                if !map.contains_key("cache_control") {
                    map.insert("cache_control".to_string(), cache_control_ephemeral(rule.ttl));
                    *remaining_slots = remaining_slots.saturating_sub(1);
                }
            }
            Some(Value::Object(map)) => {
                if resolve_rule_index(1, rule.position, rule.index).is_none() {
                    return;
                }
                if !map.contains_key("cache_control") {
                    map.insert("cache_control".to_string(), cache_control_ephemeral(rule.ttl));
                    *remaining_slots = remaining_slots.saturating_sub(1);
                }
            }
            _ => {}
        },
        CacheBreakpointTarget::Messages => {
            let Some(messages) = root.get_mut("messages").and_then(Value::as_array_mut) else {
                return;
            };
            let Some(idx) = resolve_rule_index(messages.len(), rule.position, rule.index) else {
                return;
            };
            let Some(message_map) = messages[idx].as_object_mut() else {
                return;
            };
            let Some(content) = message_map.get_mut("content") else {
                return;
            };
            if apply_cache_control_to_message_content(content, rule.ttl) {
                *remaining_slots = remaining_slots.saturating_sub(1);
            }
        }
    }
}

fn apply_cache_control_to_message_content(content: &mut Value, ttl: CacheBreakpointTtl) -> bool {
    match content {
        Value::Array(blocks) => {
            for content_idx in (0..blocks.len()).rev() {
                let Some(map) = blocks[content_idx].as_object_mut() else {
                    continue;
                };
                if map.contains_key("cache_control") {
                    continue;
                }
                map.insert("cache_control".to_string(), cache_control_ephemeral(ttl));
                return true;
            }
            false
        }
        Value::Object(map) => {
            if map.contains_key("cache_control") {
                return false;
            }
            map.insert("cache_control".to_string(), cache_control_ephemeral(ttl));
            true
        }
        _ => false,
    }
}

fn resolve_rule_index(
    len: usize,
    position: CacheBreakpointPositionKind,
    index: usize,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let idx = index.max(1);
    match position {
        CacheBreakpointPositionKind::Nth => {
            if idx > len {
                None
            } else {
                Some(idx - 1)
            }
        }
        CacheBreakpointPositionKind::LastNth => {
            if idx > len {
                None
            } else {
                Some(len - idx)
            }
        }
    }
}

fn cache_control_ephemeral(ttl: CacheBreakpointTtl) -> Value {
    let mut cache_control = serde_json::json!({
        "type": "ephemeral",
    });
    if let Some(ttl) = ttl.ttl() {
        cache_control["ttl"] = serde_json::json!(ttl);
    }
    cache_control
}

fn existing_cache_breakpoint_count(root: &serde_json::Map<String, Value>) -> usize {
    let mut count = 0usize;
    if root.contains_key("cache_control") {
        count += 1;
    }

    if let Some(tools) = root.get("tools").and_then(Value::as_array) {
        count += tools
            .iter()
            .filter_map(Value::as_object)
            .filter(|item| item.contains_key("cache_control"))
            .count();
    }

    match root.get("system") {
        Some(Value::Array(blocks)) => {
            count += blocks
                .iter()
                .filter_map(Value::as_object)
                .filter(|item| item.contains_key("cache_control"))
                .count();
        }
        Some(Value::Object(item)) => {
            if item.contains_key("cache_control") {
                count += 1;
            }
        }
        _ => {}
    }

    if let Some(messages) = root.get("messages").and_then(Value::as_array) {
        for message in messages {
            let Some(message_map) = message.as_object() else {
                continue;
            };
            let Some(content) = message_map.get("content") else {
                continue;
            };
            match content {
                Value::Array(blocks) => {
                    count += blocks
                        .iter()
                        .filter_map(Value::as_object)
                        .filter(|item| item.contains_key("cache_control"))
                        .count();
                }
                Value::Object(item) => {
                    if item.contains_key("cache_control") {
                        count += 1;
                    }
                }
                _ => {}
            }
        }
    }

    count
}

#[cfg(test)]
mod tests {
    use super::{
        CacheBreakpointPositionKind, CacheBreakpointRule, CacheBreakpointTarget, CacheBreakpointTtl,
        apply_magic_string_cache_control_triggers, ensure_cache_breakpoint_rules,
        parse_cache_breakpoint_rules,
    };
    use serde_json::json;

    #[test]
    fn parse_cache_breakpoint_rules_limits_to_four_and_normalizes() {
        let parsed = parse_cache_breakpoint_rules(Some(&json!([
            {"target":"messages","position":"nth","index":0,"ttl":"auto"},
            {"target":"system","position":"last_nth","index":2,"ttl":"5m"},
            {"target":"tools","position":"nth","index":1,"ttl":"1h"},
            {"target":"top_level","ttl":"auto"},
            {"target":"messages","position":"nth","index":3,"ttl":"5m"}
        ])));
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].index, 1);
        assert_eq!(parsed[1].target, CacheBreakpointTarget::System);
        assert_eq!(parsed[2].ttl, CacheBreakpointTtl::Ttl1h);
        assert_eq!(parsed[3].target, CacheBreakpointTarget::TopLevel);
    }

    #[test]
    fn ensure_cache_breakpoint_rules_applies_top_level_and_message_rules() {
        let mut body = json!({
            "messages": [
                {"role":"user","content":[{"type":"text","text":"m0"}]},
                {"role":"assistant","content":[{"type":"text","text":"m1"}]}
            ]
        });
        let rules = vec![
            CacheBreakpointRule {
                target: CacheBreakpointTarget::TopLevel,
                position: CacheBreakpointPositionKind::Nth,
                index: 1,
                ttl: CacheBreakpointTtl::Auto,
            },
            CacheBreakpointRule {
                target: CacheBreakpointTarget::Messages,
                position: CacheBreakpointPositionKind::LastNth,
                index: 1,
                ttl: CacheBreakpointTtl::Ttl1h,
            },
        ];

        ensure_cache_breakpoint_rules(&mut body, &rules);
        assert_eq!(body["cache_control"]["type"], json!("ephemeral"));
        assert_eq!(body["cache_control"]["ttl"], json!(null));
        assert_eq!(body["messages"][1]["content"][0]["cache_control"]["ttl"], json!("1h"));
    }

    #[test]
    fn ensure_cache_breakpoint_rules_respects_existing_breakpoint_slots() {
        let mut body = json!({
            "cache_control": {"type":"ephemeral","ttl":"1h"},
            "system": [
                {"type":"text","text":"s0","cache_control":{"type":"ephemeral","ttl":"1h"}},
                {"type":"text","text":"s1"}
            ],
            "messages": [
                {"role":"user","content":[{"type":"text","text":"m0","cache_control":{"type":"ephemeral","ttl":"1h"}}]},
                {"role":"user","content":[{"type":"text","text":"m1"}]},
                {"role":"user","content":[{"type":"text","text":"m2"}]}
            ]
        });
        let rules = vec![
            CacheBreakpointRule {
                target: CacheBreakpointTarget::TopLevel,
                position: CacheBreakpointPositionKind::Nth,
                index: 1,
                ttl: CacheBreakpointTtl::Auto,
            },
            CacheBreakpointRule {
                target: CacheBreakpointTarget::System,
                position: CacheBreakpointPositionKind::Nth,
                index: 2,
                ttl: CacheBreakpointTtl::Ttl5m,
            },
            CacheBreakpointRule {
                target: CacheBreakpointTarget::Messages,
                position: CacheBreakpointPositionKind::Nth,
                index: 2,
                ttl: CacheBreakpointTtl::Ttl5m,
            },
            CacheBreakpointRule {
                target: CacheBreakpointTarget::Messages,
                position: CacheBreakpointPositionKind::Nth,
                index: 3,
                ttl: CacheBreakpointTtl::Ttl5m,
            },
        ];

        ensure_cache_breakpoint_rules(&mut body, &rules);

        assert_eq!(body["system"][1]["cache_control"]["ttl"], json!("5m"));
        assert_eq!(body["messages"][1]["content"][0]["cache_control"], json!(null));
        assert_eq!(body["messages"][2]["content"][0]["cache_control"], json!(null));
    }

    #[test]
    fn apply_magic_string_cache_control_triggers_removes_markers_and_adds_cache_control() {
        let mut body = json!({
            "system": [
                {
                    "type":"text",
                    "text":"prefix GPROXY_MAGIC_STRING_TRIGGER_CACHING_CREATE_7D9ASD7A98SD7A9S8D79ASC98A7FNKJBVV80SCMSHDSIUCH auto suffix"
                }
            ],
            "messages": [
                {
                    "role":"user",
                    "content":[
                        {
                            "type":"text",
                            "text":"x GPROXY_MAGIC_STRING_TRIGGER_CACHING_CREATE_49VA1S5V19GR4G89W2V695G9W9GV52W95V198WV5W2FC9DF 5m y"
                        }
                    ]
                },
                {
                    "role":"assistant",
                    "content":[
                        {
                            "type":"text",
                            "text":"z GPROXY_MAGIC_STRING_TRIGGER_CACHING_CREATE_1FAS5GV9R5H29T5Y2J9584K6O95M2NBVW52C95CX984FRJY 1h w"
                        }
                    ]
                }
            ]
        });

        apply_magic_string_cache_control_triggers(&mut body);

        let system_text = body["system"][0]["text"].as_str().unwrap_or_default();
        let message_5m_text = body["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        let message_1h_text = body["messages"][1]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();

        assert!(!system_text.contains("GPROXY_MAGIC_STRING_TRIGGER_CACHING_CREATE_"));
        assert!(!message_5m_text.contains("GPROXY_MAGIC_STRING_TRIGGER_CACHING_CREATE_"));
        assert!(!message_1h_text.contains("GPROXY_MAGIC_STRING_TRIGGER_CACHING_CREATE_"));
        assert_eq!(body["system"][0]["cache_control"]["type"], json!("ephemeral"));
        assert_eq!(body["system"][0]["cache_control"]["ttl"], json!(null));
        assert_eq!(body["messages"][0]["content"][0]["cache_control"]["ttl"], json!("5m"));
        assert_eq!(body["messages"][1]["content"][0]["cache_control"]["ttl"], json!("1h"));
    }

    #[test]
    fn apply_magic_string_cache_control_triggers_keeps_existing_cache_control() {
        let mut body = json!({
            "messages": [
                {
                    "role":"user",
                    "content":[
                        {
                            "type":"text",
                            "text":"GPROXY_MAGIC_STRING_TRIGGER_CACHING_CREATE_1FAS5GV9R5H29T5Y2J9584K6O95M2NBVW52C95CX984FRJY 1h",
                            "cache_control":{"type":"ephemeral","ttl":"5m"}
                        }
                    ]
                }
            ]
        });

        apply_magic_string_cache_control_triggers(&mut body);

        assert_eq!(body["messages"][0]["content"][0]["cache_control"]["ttl"], json!("5m"));
        assert_eq!(body["messages"][0]["content"][0]["text"], json!(""));
    }
}
