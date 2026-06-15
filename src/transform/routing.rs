//! Compiled routing rules (§8-B2 `routing_rules`) and the transform-dispatch
//! decision (§6.1): passthrough / transform_to / local / unsupported.

use serde_json::Value;

use crate::protocol::{ContentGenerationKind, Operation, OperationKey, OperationKind, Provider};
use crate::store::persistence::records::RoutingRule;

/// `routing_rules.implementation`, parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleImpl {
    Passthrough,
    TransformTo,
    Local,
    Unsupported,
}

/// A routing-rule row with its string fields parsed into protocol enums.
#[derive(Debug, Clone)]
pub struct CompiledRoutingRule {
    pub operation: Operation,
    pub kind: OperationKind,
    pub implementation: RuleImpl,
    pub dest_operation: Option<Operation>,
    pub dest_kind: Option<OperationKind>,
}

/// Parse enabled rows in `sort_order`. Unparsable rows are skipped with a
/// warning — bad config must not take the snapshot down.
pub fn compile(rows: &[RoutingRule]) -> Vec<CompiledRoutingRule> {
    let mut rows: Vec<&RoutingRule> = rows.iter().filter(|r| r.enabled).collect();
    rows.sort_by_key(|r| r.sort_order);
    let mut out = Vec::new();
    for row in rows {
        match compile_row(row) {
            Some(rule) => out.push(rule),
            None => tracing::warn!(
                rule_id = row.id,
                provider_id = row.provider_id,
                "skipping unparsable routing rule"
            ),
        }
    }
    out
}

fn compile_row(row: &RoutingRule) -> Option<CompiledRoutingRule> {
    Some(CompiledRoutingRule {
        operation: parse_str(&row.operation)?,
        kind: parse_str(&row.kind)?,
        implementation: match row.implementation.as_str() {
            "passthrough" => RuleImpl::Passthrough,
            "transform_to" => RuleImpl::TransformTo,
            "local" => RuleImpl::Local,
            "unsupported" => RuleImpl::Unsupported,
            _ => return None,
        },
        dest_operation: match &row.dest_operation {
            Some(s) => Some(parse_str(s)?),
            None => None,
        },
        dest_kind: match &row.dest_kind {
            Some(s) => Some(parse_str(s)?),
            None => None,
        },
    })
}

/// Protocol enums all serde-rename to snake_case strings (`"claude_messages"`,
/// `"open_ai"`, …) — reuse that as the single parse path. `OperationKind` is
/// `#[serde(untagged)]`, so a plain string tries ContentGenerationKind first,
/// then Provider, exactly matching the §8 kind vocabulary.
fn parse_str<T: serde::de::DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_value(Value::String(s.to_owned())).ok()
}

/// The dispatch decision for one `(source op, target channel kind)` pairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDecision {
    Passthrough,
    TransformTo(OperationKey),
    Local,
    Unsupported,
}

/// Decide how to service `source` on a channel whose native content kind is
/// `target_kind`, at REQUEST time. Routing is driven entirely by stored rules:
/// an explicit rule wins; **no matching rule means `Unsupported`**. Channel
/// defaults are materialized into real rules at provider creation (see
/// [`crate::api::routing::seed_default_routing`]) — they are not recomputed here.
pub fn decide(
    rules: &[CompiledRoutingRule],
    source: OperationKey,
    target_kind: ContentGenerationKind,
) -> RoutingDecision {
    if let Some(rule) = rules
        .iter()
        .find(|r| r.operation == source.operation && r.kind == source.kind)
    {
        return match rule.implementation {
            RuleImpl::Passthrough => RoutingDecision::Passthrough,
            RuleImpl::Local => RoutingDecision::Local,
            RuleImpl::Unsupported => RoutingDecision::Unsupported,
            RuleImpl::TransformTo => RoutingDecision::TransformTo(OperationKey {
                operation: rule.dest_operation.unwrap_or(source.operation),
                kind: rule
                    .dest_kind
                    .unwrap_or_else(|| default_target_kind(source, target_kind)),
            }),
        };
    }
    RoutingDecision::Unsupported
}

/// The computed default decision for one `(source, target_kind)` cell. Used to
/// MATERIALIZE defaults at provider creation / reset — never at request time.
/// native-passthrough, else auto-transform to the channel's native kind;
/// count_tokens on an openai-family target is served locally (§6.3).
pub fn default_decision(
    source: OperationKey,
    target_kind: ContentGenerationKind,
) -> RoutingDecision {
    // §6.3: openai-family channels have no count endpoint worth hitting by
    // default — serve locally. (The operator can opt into passthrough via a rule.)
    if source.operation == Operation::CountTokens && target_kind.provider() == Provider::OpenAi {
        return RoutingDecision::Local;
    }
    if is_native(source.kind, target_kind) {
        return RoutingDecision::Passthrough;
    }
    RoutingDecision::TransformTo(OperationKey {
        operation: source.operation,
        kind: default_target_kind(source, target_kind),
    })
}

/// Native = the inbound wire shape is already what the channel speaks.
fn is_native(source_kind: OperationKind, target: ContentGenerationKind) -> bool {
    match source_kind {
        OperationKind::ContentGeneration(k) => k == target,
        // Non-content operations are provider-family-shaped (e.g. OpenAI
        // embeddings) and native on any channel of the same family — this is
        // exactly M1's implicit passthrough behavior, preserved.
        OperationKind::Provider(p) => p == target.provider(),
    }
}

fn default_target_kind(source: OperationKey, target: ContentGenerationKind) -> OperationKind {
    if source.operation.is_content_generation() {
        OperationKind::ContentGeneration(target)
    } else {
        OperationKind::Provider(target.provider())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cg(op: Operation, k: ContentGenerationKind) -> OperationKey {
        OperationKey::content_generation(op, k)
    }

    #[test]
    fn default_decisions() {
        use ContentGenerationKind as K;
        let src = cg(Operation::GenerateContent, K::ClaudeMessages);
        // same kind → passthrough
        assert_eq!(
            default_decision(src, K::ClaudeMessages),
            RoutingDecision::Passthrough
        );
        // cross kind → auto transform to channel native
        assert_eq!(
            default_decision(src, K::OpenAiChatCompletions),
            RoutingDecision::TransformTo(cg(Operation::GenerateContent, K::OpenAiChatCompletions))
        );
        // openai-family non-content op on an openai-kind channel → native
        let emb = OperationKey::provider(
            Operation::CreateEmbedding,
            crate::protocol::Provider::OpenAi,
        );
        assert_eq!(
            default_decision(emb, K::OpenAiChatCompletions),
            RoutingDecision::Passthrough
        );
    }

    #[test]
    fn no_rule_is_unsupported() {
        use ContentGenerationKind as K;
        // At request time, an unseeded cell (no matching rule) is unsupported.
        let src = cg(Operation::GenerateContent, K::ClaudeMessages);
        assert_eq!(
            decide(&[], src, K::ClaudeMessages),
            RoutingDecision::Unsupported
        );
    }

    #[test]
    fn explicit_rule_wins() {
        let rule = CompiledRoutingRule {
            operation: Operation::GenerateContent,
            kind: OperationKind::ContentGeneration(ContentGenerationKind::ClaudeMessages),
            implementation: RuleImpl::Unsupported,
            dest_operation: None,
            dest_kind: None,
        };
        let src = cg(
            Operation::GenerateContent,
            ContentGenerationKind::ClaudeMessages,
        );
        assert_eq!(
            decide(&[rule], src, ContentGenerationKind::ClaudeMessages),
            RoutingDecision::Unsupported
        );
    }
}
