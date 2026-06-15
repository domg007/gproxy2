//! Cross-target DTO and core logic for `GET /admin/providers/{id}/routing-rules`.
//!
//! [`routing_view`] computes the full routing picture for a provider: the 3×4
//! content-generation `(Operation, ContentGenerationKind)` decision matrix (via
//! [`crate::transform::routing::decide`]), each cell tagged `"default"` or
//! `"custom"` and carrying the stored rule's `id` when custom, plus any extra
//! stored rules that target an `(operation, kind)` pair outside the default
//! matrix. This is the single source the Console routing table renders and
//! edits from — no separate "effective" endpoint, no client-side join.

use std::collections::HashMap;

use serde::Serialize;

use crate::api::error::ApiError;
use crate::app::AppState;
use crate::protocol::{ContentGenerationKind, Operation, OperationKey, OperationKind};
use crate::store::persistence::records::RoutingRule;
use crate::transform::routing::{self, RoutingDecision};

/// One row of the routing view: a default-matrix cell or an extra stored rule.
#[derive(Debug, Serialize)]
pub struct RoutingViewRow {
    /// Snake-case `Operation` (e.g. `"generate_content"`).
    pub operation: String,
    /// Snake-case inbound kind (e.g. `"claude_messages"`).
    pub kind: String,
    /// `"passthrough"` | `"transform_to"` | `"local"` | `"unsupported"`.
    pub implementation: String,
    /// For `transform_to`: snake-case dest `Operation`.
    pub dest_operation: Option<String>,
    /// For `transform_to`: snake-case dest kind.
    pub dest_kind: Option<String>,
    /// `"default"` — computed, no stored rule; `"custom"` — a stored rule drives it.
    pub source: String,
    /// Stored rule id when `source == "custom"` (the edit/delete handle); else `null`.
    pub id: Option<i64>,
    /// Stored rule sort order when custom; else `null`.
    pub sort_order: Option<i64>,
    /// `true` for the default 3×4 matrix cells; `false` for extra rules that
    /// target a pair outside it (those are deleted, not reset-to-default).
    pub cell: bool,
}

/// Serialize any `T: serde::Serialize` to its JSON-string representation
/// (the `serde(rename_all = "snake_case")` path for protocol enums).
fn to_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|j| j.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Compute the routing view for `provider_id`: the default 3×4 matrix (each cell
/// tagged default/custom with the driving rule's id) followed by any extra
/// stored rules outside the matrix.
///
/// Errors:
/// - `NotFound` — provider not found in persistence.
/// - `BadRequest` — provider's channel id is unknown (mis-configured).
/// - `Internal` — persistence failure.
pub async fn routing_view(
    state: &AppState,
    provider_id: i64,
) -> Result<Vec<RoutingViewRow>, ApiError> {
    // 1. Load provider.
    let provider = state
        .persistence
        .get_provider(provider_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("provider {provider_id} not found")))?;

    // 2. Resolve channel → target_kind.
    let channel = state
        .channels
        .get(&provider.channel)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown channel: {}", provider.channel)))?;
    let target_kind = channel.target_kind();

    // 3. Load stored rules; compile (enabled, sort_order) for decide().
    let stored = state
        .persistence
        .list_routing_rules(provider_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let compiled = routing::compile(&stored);

    // Index enabled stored rules by their (operation, kind) strings, in the same
    // precedence decide() uses (sort_order, then id) so the id we attach to a
    // cell is the rule that actually drives it.
    let mut enabled: Vec<&RoutingRule> = stored.iter().filter(|r| r.enabled).collect();
    enabled.sort_by_key(|r| (r.sort_order, r.id));
    let mut rule_by_cell: HashMap<(&str, &str), &RoutingRule> = HashMap::new();
    for r in &enabled {
        rule_by_cell
            .entry((r.operation.as_str(), r.kind.as_str()))
            .or_insert(r);
    }

    // 4. The default 3×4 content-generation matrix.
    const OPS: [Operation; 3] = [
        Operation::GenerateContent,
        Operation::StreamGenerateContent,
        Operation::CountTokens,
    ];
    const KINDS: [ContentGenerationKind; 4] = [
        ContentGenerationKind::OpenAiResponses,
        ContentGenerationKind::OpenAiChatCompletions,
        ContentGenerationKind::ClaudeMessages,
        ContentGenerationKind::GeminiGenerateContent,
    ];

    let mut rows = Vec::with_capacity(OPS.len() * KINDS.len());
    let mut cell_keys: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();

    for op in OPS {
        for cgk in KINDS {
            let source_key = OperationKey {
                operation: op,
                kind: OperationKind::ContentGeneration(cgk),
            };
            let decision = routing::decide(&compiled, source_key, target_kind);
            let (implementation, dest_operation, dest_kind) = decision_strs(decision);

            let op_str = to_str(&op);
            let kind_str = to_str(&cgk);
            let rule = rule_by_cell.get(&(op_str.as_str(), kind_str.as_str()));
            let id = rule.map(|r| r.id);
            let sort_order = rule.map(|r| r.sort_order);
            let is_custom = rule.is_some();
            cell_keys.insert((op_str.clone(), kind_str.clone()));

            rows.push(RoutingViewRow {
                operation: op_str,
                kind: kind_str,
                implementation,
                dest_operation,
                dest_kind,
                source: if is_custom { "custom" } else { "default" }.to_owned(),
                id,
                sort_order,
                cell: true,
            });
        }
    }

    // 5. Extra stored rules whose (operation, kind) falls outside the matrix.
    let mut seen_extra: std::collections::HashSet<(&str, &str)> = std::collections::HashSet::new();
    for r in &enabled {
        let key = (r.operation.clone(), r.kind.clone());
        if cell_keys.contains(&key) || !seen_extra.insert((r.operation.as_str(), r.kind.as_str())) {
            continue;
        }
        rows.push(RoutingViewRow {
            operation: r.operation.clone(),
            kind: r.kind.clone(),
            implementation: r.implementation.clone(),
            dest_operation: r.dest_operation.clone(),
            dest_kind: r.dest_kind.clone(),
            source: "custom".to_owned(),
            id: Some(r.id),
            sort_order: Some(r.sort_order),
            cell: false,
        });
    }

    Ok(rows)
}

/// Map a [`RoutingDecision`] to `(implementation, dest_operation, dest_kind)` strings.
fn decision_strs(decision: RoutingDecision) -> (String, Option<String>, Option<String>) {
    match decision {
        RoutingDecision::Passthrough => ("passthrough".to_owned(), None, None),
        RoutingDecision::Local => ("local".to_owned(), None, None),
        RoutingDecision::Unsupported => ("unsupported".to_owned(), None, None),
        RoutingDecision::TransformTo(dest) => {
            let dest_kind_str = match dest.kind {
                OperationKind::ContentGeneration(k) => to_str(&k),
                OperationKind::Provider(p) => to_str(&p),
            };
            (
                "transform_to".to_owned(),
                Some(to_str(&dest.operation)),
                Some(dest_kind_str),
            )
        }
    }
}
