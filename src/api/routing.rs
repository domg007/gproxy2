//! Default routing-rule seeding for a provider.
//!
//! [`seed_default_routing`] computes the channel's default decisions for the 3×4
//! content-generation `(Operation, ContentGenerationKind)` matrix (via
//! [`crate::transform::routing::decide`] with no stored rules) and writes them as
//! real `routing_rules` rows — creating them when absent, overwriting them when
//! present. It runs once at provider creation (so `GET /routing-rules` is a plain
//! load) and again on the explicit "reset defaults" action (e.g. after upgrading
//! the routing logic). Extra rules outside the matrix are left untouched.

use crate::api::error::ApiError;
use crate::app::AppState;
use crate::protocol::{ContentGenerationKind, Operation, OperationKey, OperationKind};
use crate::store::persistence::records::RoutingRuleInput;
use crate::transform::routing::{self, RoutingDecision};

/// Serialize a `serde`-enum to its snake-case string form.
fn to_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|j| j.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Map a [`RoutingDecision`] to `(implementation, dest_operation, dest_kind)`.
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

/// Seed (create or overwrite) the 3×4 default content-generation routing rules
/// for `provider_id`, computed from its channel. Idempotent: an existing rule for
/// a cell is updated in place; a missing one is created. Rules outside the matrix
/// are left as-is.
///
/// Errors: `NotFound` (provider gone), `BadRequest` (unknown channel), `Internal`.
pub async fn seed_default_routing(state: &AppState, provider_id: i64) -> Result<(), ApiError> {
    let provider = state
        .persistence
        .get_provider(provider_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("provider {provider_id} not found")))?;

    let channel = state
        .channels
        .get(&provider.channel)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown channel: {}", provider.channel)))?;
    let target_kind = channel.target_kind();

    // Existing rows, so a reset overwrites a cell's rule in place (keep its id).
    let existing = state
        .persistence
        .list_routing_rules(provider_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

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

    let mut sort_order = 0i64;
    for op in OPS {
        for cgk in KINDS {
            let key = OperationKey {
                operation: op,
                kind: OperationKind::ContentGeneration(cgk),
            };
            // Pure default: decide() with no stored rules.
            let (implementation, dest_operation, dest_kind) =
                decision_strs(routing::decide(&[], key, target_kind));
            let operation = to_str(&op);
            let kind = to_str(&cgk);
            let id = existing
                .iter()
                .find(|r| r.operation == operation && r.kind == kind)
                .map(|r| r.id);

            state
                .persistence
                .upsert_routing_rule(RoutingRuleInput {
                    id,
                    provider_id,
                    operation,
                    kind,
                    implementation,
                    dest_operation,
                    dest_kind,
                    sort_order,
                    enabled: true,
                })
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?;
            sort_order += 1;
        }
    }

    Ok(())
}
