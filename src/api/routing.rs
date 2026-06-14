//! Cross-target DTO and core logic for `GET /admin/providers/{id}/routing-rules/effective`.
//!
//! [`effective_routes`] computes the full 3×4 op×kind decision matrix for a
//! provider by running [`crate::transform::routing::decide`] over every
//! content-generation `(Operation, ContentGenerationKind)` pair and tagging
//! each cell as `"default"` or `"override"` depending on whether a stored rule
//! matched it.

use serde::Serialize;

use crate::api::error::ApiError;
use crate::app::AppState;
use crate::protocol::{ContentGenerationKind, Operation, OperationKind};
use crate::transform::routing::{self, RoutingDecision};

/// Response row for one `(operation, inbound kind)` cell of the routing matrix.
#[derive(Debug, Serialize)]
pub struct EffectiveRoute {
    /// Snake-case `Operation` (e.g. `"generate_content"`).
    pub operation: String,
    /// Snake-case inbound `ContentGenerationKind` (e.g. `"claude_messages"`).
    pub kind: String,
    /// `"passthrough"` | `"transform_to"` | `"local"` | `"unsupported"`.
    pub implementation: String,
    /// For `transform_to`: snake-case dest `Operation`.
    pub dest_operation: Option<String>,
    /// For `transform_to`: snake-case dest `ContentGenerationKind`.
    pub dest_kind: Option<String>,
    /// `"default"` — no stored rule matched; `"override"` — a stored enabled
    /// rule matched this exact `(operation, kind)` pair.
    pub source: String,
}

/// Serialize any `T: serde::Serialize` to its JSON-string representation
/// (the `serde(rename_all = "snake_case")` path for protocol enums).
fn to_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|j| j.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Compute the effective routing matrix for `provider_id`.
///
/// Enumerates the 3×4 content-generation `(Operation, ContentGenerationKind)`
/// matrix, calls [`routing::decide`] for each cell, and tags the result as
/// `"default"` or `"override"`. Resolves the channel's native kind via
/// `ChannelRegistry`.
///
/// Errors:
/// - `NotFound` — provider not found in persistence.
/// - `BadRequest` — provider's channel id is unknown (mis-configured).
/// - `Internal` — persistence failure.
pub async fn effective_routes(
    state: &AppState,
    provider_id: i64,
) -> Result<Vec<EffectiveRoute>, ApiError> {
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

    // 3. Load and compile stored routing rules.
    let stored = state
        .persistence
        .list_routing_rules(provider_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let compiled = routing::compile(&stored);

    // 4. Build a quick-lookup set of (operation_str, kind_str) for stored rules
    //    (only enabled ones are compiled, but we still need to know if a rule
    //    *was* stored + enabled for the override tag — compile already filters
    //    disabled rows, so we can derive "override" from compiled membership).
    //
    //    Strategy: a cell is "override" iff the compiled rules list contains a
    //    rule whose (operation, kind) == (source_op, source_cgk).
    use crate::protocol::OperationKey;

    // 5. Enumerate the 3 × 4 matrix.
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

    for op in OPS {
        for cgk in KINDS {
            let source_key = OperationKey {
                operation: op,
                kind: OperationKind::ContentGeneration(cgk),
            };
            let decision = routing::decide(&compiled, source_key, target_kind);

            // "override" iff a compiled (enabled) rule matched this exact cell.
            let is_override = compiled
                .iter()
                .any(|r| r.operation == op && r.kind == OperationKind::ContentGeneration(cgk));

            let (implementation, dest_operation, dest_kind) = match decision {
                RoutingDecision::Passthrough => ("passthrough".to_owned(), None, None),
                RoutingDecision::Local => ("local".to_owned(), None, None),
                RoutingDecision::Unsupported => ("unsupported".to_owned(), None, None),
                RoutingDecision::TransformTo(dest) => {
                    let dest_op_str = to_str(&dest.operation);
                    let dest_kind_str = match dest.kind {
                        OperationKind::ContentGeneration(k) => to_str(&k),
                        OperationKind::Provider(p) => to_str(&p),
                    };
                    (
                        "transform_to".to_owned(),
                        Some(dest_op_str),
                        Some(dest_kind_str),
                    )
                }
            };

            rows.push(EffectiveRoute {
                operation: to_str(&op),
                kind: to_str(&cgk),
                implementation,
                dest_operation,
                dest_kind,
                source: if is_override { "override" } else { "default" }.to_owned(),
            });
        }
    }

    Ok(rows)
}
