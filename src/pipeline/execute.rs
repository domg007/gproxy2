//! The generic request orchestrator (§6.3). Sequences the already-separated
//! steps for both routing modes; stream & non-stream share every step and
//! diverge only at the body tail inside [`failover`](crate::pipeline::failover).

use std::sync::Arc;

use crate::app::AppState;
use crate::pipeline::context::{Candidate, RequestCtx, RoutingMode};
use crate::pipeline::error::PipelineError;
use crate::pipeline::outcome::ExecOutcome;
use crate::pipeline::{auth, balance, classify, failover, preprocess, route};

/// Drive one request to an [`ExecOutcome`].
pub async fn execute(state: &AppState, mut ctx: RequestCtx) -> Result<ExecOutcome, PipelineError> {
    let cp = state.cp();

    // auth (401 short-circuit before any upstream candidate is built)
    ctx.identity = Some(auth::authenticate(&cp, &ctx.headers)?);

    // classify
    let classified = classify::classify(&ctx.method, &ctx.path, &ctx.body)?;
    ctx.op = Some(classified.op);
    ctx.stream = classified.stream;

    // resolve candidates per routing mode
    let candidates = match &ctx.mode {
        RoutingMode::Aggregated => {
            let route_name = preprocess::preprocess(&cp, &ctx)?;
            let resolved = route::route(&cp, &route_name)?;
            let cands = balance::candidates(&cp, resolved, state.cache.as_ref(), None)?;
            ctx.route_name = Some(route_name);
            cands
        }
        RoutingMode::Scoped { provider } => scoped_candidates(&cp, provider, &ctx)?,
    };

    // Candidates own their Arcs; drop the snapshot guard before the (possibly
    // long-lived, streaming) upstream call so it doesn't pin the old snapshot
    // across an invalidation/swap.
    drop(cp);

    failover::run_failover(state, &ctx, &candidates).await
}

/// Scoped mode (`/{provider}/v1/...`): bypass routing, hit the named provider
/// directly. Provider must exist + be enabled; model validation is lax in M1.
fn scoped_candidates(
    cp: &crate::app::snapshot::ControlPlaneSnapshot,
    provider_name: &str,
    ctx: &RequestCtx,
) -> Result<Vec<Candidate>, PipelineError> {
    let provider = cp
        .providers_by_name
        .get(provider_name)
        .filter(|p| p.enabled)
        .ok_or_else(|| PipelineError::UnknownProvider(provider_name.to_string()))?;

    let model = classify::peek_model(&ctx.body).unwrap_or_default();
    let creds = cp
        .credentials_by_provider
        .get(&provider.id)
        .filter(|c| !c.is_empty())
        .ok_or(PipelineError::NoCredentials)?;

    Ok(creds
        .iter()
        .map(|cred| Candidate {
            provider: Arc::clone(provider),
            credential: Arc::clone(cred),
            upstream_model_id: model.clone(),
        })
        .collect())
}
