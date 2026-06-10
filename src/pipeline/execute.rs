//! The generic request orchestrator (§6.3). Sequences the already-separated
//! steps for both routing modes; stream & non-stream share every step and
//! diverge only at the body tail inside [`failover`](crate::pipeline::failover).

use std::sync::Arc;

use crate::app::AppState;
use crate::pipeline::context::{Candidate, RequestCtx, RoutingMode};
use crate::pipeline::error::PipelineError;
use crate::pipeline::local_ops::{self, ModelEntry};
use crate::pipeline::outcome::ExecOutcome;
use crate::pipeline::{auth, authz, balance, classify, failover, ingress, preprocess, route};
use crate::protocol::Operation;
use crate::util::time::unix_now;

/// Drive one request to an [`ExecOutcome`].
pub async fn execute(state: &AppState, mut ctx: RequestCtx) -> Result<ExecOutcome, PipelineError> {
    let cp = state.cp();

    // auth (401 short-circuit before any upstream candidate is built)
    ctx.identity = Some(auth::authenticate(&cp, &ctx.headers, ctx.query.as_deref())?);

    // Part 1 — global blacklist: strip caller creds / cookies / hop-by-hop once,
    // centrally, after auth and before any channel sees the request (the
    // per-channel allow-list runs later in Channel::prepare).
    ingress::apply_global_blacklist(&mut ctx);

    // classify
    let classified = classify::classify(&ctx.method, &ctx.path, &ctx.headers, &ctx.body)?;
    ctx.op = Some(classified.op);
    ctx.stream = classified.stream;

    // Aggregated models surface (§6.3): the gateway's own view — alias + route
    // names — served before preprocess/route (there is no model to route) and
    // never touching an upstream.
    if matches!(ctx.mode, RoutingMode::Aggregated)
        && matches!(
            classified.op.operation,
            Operation::ListModels | Operation::GetModel
        )
    {
        return aggregated_models(&cp, &ctx);
    }

    // resolve candidates per routing mode, with authz (§8-C) on the canonical
    // name BEFORE any candidate is built. The snapshot guard `cp` is held
    // across authorize's await — that's only a sub-millisecond cache incr, not
    // the upstream call the M2 invariant guards against.
    let candidates = match &ctx.mode {
        RoutingMode::Aggregated => {
            let route_name = preprocess::preprocess(&cp, &ctx)?;
            let resolved = route::route(&cp, &route_name)?;
            let identity = ctx.identity.as_ref().expect("auth ran first");
            authz::authorize(&cp, state.cache.as_ref(), identity, &route_name, unix_now()).await?;
            let cands = balance::candidates(&cp, resolved, state.cache.as_ref(), None)?;
            ctx.route_name = Some(route_name);
            cands
        }
        RoutingMode::Scoped { provider } => {
            let provider = cp
                .providers_by_name
                .get(provider.as_str())
                .filter(|p| p.enabled)
                .ok_or_else(|| PipelineError::UnknownProvider(provider.clone()))?;
            let identity = ctx.identity.as_ref().expect("auth ran first");
            authz::authorize(
                &cp,
                state.cache.as_ref(),
                identity,
                &provider.name,
                unix_now(),
            )
            .await?;
            scoped_candidates(&cp, provider, &ctx)?
        }
    };

    // Candidates own their Arcs; drop the snapshot guard before the (possibly
    // long-lived, streaming) upstream call so it doesn't pin the old snapshot
    // across an invalidation/swap.
    drop(cp);

    failover::run_failover(state, &ctx, &candidates).await
}

/// Scoped mode (`/{provider}/v1/...`): bypass routing, hit the named provider
/// directly. Provider must exist + be enabled; model validation is lax (M1
/// behavior kept) — but a known variant suffix strips to its base as the
/// upstream model (§8-B), with the body/path rewrite done downstream by
/// `transform::request_parts`.
fn scoped_candidates(
    cp: &crate::app::snapshot::ControlPlaneSnapshot,
    provider: &Arc<crate::store::persistence::records::Provider>,
    ctx: &RequestCtx,
) -> Result<Vec<Candidate>, PipelineError> {
    // Requested name: body peek, else path-embedded (gemini `models/{id}:verb`,
    // models GETs). Process filters still see this ORIGINAL full name (§8-B) —
    // only upstream_model_id is stripped.
    let requested = classify::peek_model(&ctx.body)
        .or_else(|| classify::path_model_id(&ctx.path))
        .unwrap_or_default();
    let model = cp
        .variant_base_by_provider
        .get(&provider.id)
        .and_then(|idx| idx.get(&requested))
        .cloned()
        .unwrap_or(requested);
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

/// Serve aggregated ListModels/GetModel from the snapshot: alias names + route
/// names ARE the model list, filtered to what the caller's permission union
/// allows. Non-permitted GetModel 404s identically to missing (no existence
/// leak).
fn aggregated_models(
    cp: &crate::app::snapshot::ControlPlaneSnapshot,
    ctx: &RequestCtx,
) -> Result<ExecOutcome, PipelineError> {
    let op = ctx.op.expect("classified");
    let family = op.provider_family();
    let identity = ctx.identity.as_ref().expect("auth ran first");
    let known = |id: &str| cp.alias_to_route.contains_key(id) || cp.routes_by_name.contains_key(id);

    let body = match op.operation {
        Operation::ListModels => {
            let mut ids: Vec<&String> = cp
                .alias_to_route
                .keys()
                .chain(cp.routes_by_name.keys())
                .filter(|id| authz::permitted(cp, identity, id))
                .collect();
            ids.sort();
            ids.dedup();
            let entries: Vec<ModelEntry> = ids
                .into_iter()
                .map(|id| ModelEntry {
                    id: id.clone(),
                    display_name: None,
                })
                .collect();
            local_ops::render_model_list(family, &entries)
        }
        _ => {
            let id = classify::path_model_id(&ctx.path).ok_or(PipelineError::UnsupportedPath)?;
            if !known(&id) || !authz::permitted(cp, identity, &id) {
                return Err(PipelineError::UnknownRoute(id));
            }
            local_ops::render_model(
                family,
                &ModelEntry {
                    id,
                    display_name: None,
                },
            )
        }
    };

    Ok(local_ops::json_outcome(http::StatusCode::OK, body))
}
