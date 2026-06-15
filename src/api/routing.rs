//! Default routing-rule seeding for a provider.
//!
//! [`seed_default_routing`] materializes a provider's full *executable* routing
//! surface as real `routing_rules` rows: for every `(operation, inbound kind)`
//! the proxy can serve on the provider's channel, it computes the channel default
//! ([`crate::transform::routing::default_decision`]) and writes it. Pairs that
//! aren't executable (no wired transform) are skipped — at request time a missing
//! rule means `Unsupported`, so the stored rules are the whole contract. Runs at
//! provider creation and on the explicit "reset defaults" action.

use crate::api::error::ApiError;
use crate::channel::registry::ChannelRegistry;
use crate::protocol::{
    ContentGenerationKind, Operation, OperationGroup, OperationKey, OperationKind, Provider,
};
use crate::store::persistence::PersistenceBackend;
use crate::store::persistence::records::RoutingRuleInput;
use crate::transform::routing::{self, RoutingDecision};

/// Every operation the proxy classifies. Seeding walks all of them.
const ALL_OPS: [Operation; 10] = [
    Operation::ListModels,
    Operation::GetModel,
    Operation::CountTokens,
    Operation::GenerateContent,
    Operation::StreamGenerateContent,
    Operation::CreateImage,
    Operation::EditImage,
    Operation::CreateEmbedding,
    Operation::CompactContent,
    Operation::CreateConversation,
];

const CG_KINDS: [ContentGenerationKind; 4] = [
    ContentGenerationKind::OpenAiResponses,
    ContentGenerationKind::OpenAiChatCompletions,
    ContentGenerationKind::ClaudeMessages,
    ContentGenerationKind::GeminiGenerateContent,
];
const PROVIDER_KINDS: [Provider; 3] = [Provider::OpenAi, Provider::Claude, Provider::Gemini];

/// Inbound wire kinds an operation accepts: content generation speaks the four
/// content-gen formats; every other operation (count_tokens, embeddings, images,
/// compact, models, …) is keyed by the three provider families — matching how
/// requests are classified in `protocol::endpoint`.
fn inbound_kinds(op: Operation) -> Vec<OperationKind> {
    match op.group() {
        OperationGroup::GenerateContent => CG_KINDS
            .into_iter()
            .map(OperationKind::ContentGeneration)
            .collect(),
        _ => PROVIDER_KINDS
            .into_iter()
            .map(OperationKind::Provider)
            .collect(),
    }
}

/// A default decision is executable iff it's passthrough/local, or a transform
/// whose pair is actually wired.
fn is_executable(source: OperationKey, decision: &RoutingDecision) -> bool {
    match decision {
        RoutingDecision::Passthrough | RoutingDecision::Local => true,
        RoutingDecision::Unsupported => false,
        RoutingDecision::TransformTo(dest) => crate::transform::resolve(source, *dest)
            .map(crate::transform::dispatch::is_wired)
            .unwrap_or(false),
    }
}

/// Serialize a `serde`-enum to its snake-case string form.
fn to_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|j| j.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Map a [`RoutingDecision`] to `(implementation, dest_operation, dest_kind)`.
fn decision_strs(decision: &RoutingDecision) -> (String, Option<String>, Option<String>) {
    match decision {
        RoutingDecision::Passthrough => ("passthrough".to_owned(), None, None),
        RoutingDecision::Local => ("local".to_owned(), None, None),
        RoutingDecision::Unsupported => ("unsupported".to_owned(), None, None),
        RoutingDecision::TransformTo(dest) => (
            "transform_to".to_owned(),
            Some(to_str(&dest.operation)),
            Some(to_str(&dest.kind)),
        ),
    }
}

/// Seed the executable default routing rules for `provider_id`, computed from
/// its channel. `overwrite=false` fills only cells that have no rule yet (keeps
/// existing/edited rows); `overwrite=true` recomputes and overwrites every
/// default cell (the "reset defaults" semantics). Extra rules outside the matrix
/// are always left untouched.
///
/// Errors: `NotFound` (provider gone), `BadRequest` (unknown channel), `Internal`.
pub async fn seed_default_routing(
    persistence: &dyn PersistenceBackend,
    channels: &ChannelRegistry,
    provider_id: i64,
    overwrite: bool,
) -> Result<(), ApiError> {
    let provider = persistence
        .get_provider(provider_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound(format!("provider {provider_id} not found")))?;

    let channel = channels
        .get(&provider.channel)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown channel: {}", provider.channel)))?;
    let target_kind = channel.target_kind();

    let existing = persistence
        .list_routing_rules(provider_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let mut sort_order = 0i64;
    for op in ALL_OPS {
        for kind in inbound_kinds(op) {
            let source = OperationKey {
                operation: op,
                kind,
            };
            let decision = routing::default_decision(source, target_kind);
            if !is_executable(source, &decision) {
                continue; // not wired → no rule → Unsupported at request time
            }
            let (implementation, dest_operation, dest_kind) = decision_strs(&decision);
            let operation = to_str(&op);
            let kind_str = to_str(&kind);
            let existing_id = existing
                .iter()
                .find(|r| r.operation == operation && r.kind == kind_str)
                .map(|r| r.id);
            if existing_id.is_some() && !overwrite {
                continue; // keep the existing/edited rule
            }

            persistence
                .upsert_routing_rule(RoutingRuleInput {
                    id: existing_id,
                    provider_id,
                    operation,
                    kind: kind_str,
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
