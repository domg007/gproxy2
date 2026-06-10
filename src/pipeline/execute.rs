//! The generic request orchestrator (§6.3). Sequences the already-separated
//! steps for both routing modes; stream & non-stream share every step and
//! diverge only at the body tail inside [`failover`](crate::pipeline::failover).

use std::sync::Arc;

use crate::app::AppState;
use crate::pipeline::context::{Candidate, RequestCtx, RoutingMode};
use crate::pipeline::error::PipelineError;
use crate::pipeline::local_ops::{self, ModelEntry};
use crate::pipeline::outcome::{ExecOutcome, ResponseBody};
use crate::pipeline::{auth, balance, classify, failover, ingress, preprocess, route};
use crate::protocol::Operation;

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
/// directly. Provider must exist + be enabled; model validation is lax (M1
/// behavior kept) — but a known variant suffix strips to its base as the
/// upstream model (§8-B), with the body/path rewrite done downstream by
/// `transform::request_parts`.
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
/// names ARE the model list. GetModel is an existence check on the same set.
fn aggregated_models(
    cp: &crate::app::snapshot::ControlPlaneSnapshot,
    ctx: &RequestCtx,
) -> Result<ExecOutcome, PipelineError> {
    let op = ctx.op.expect("classified");
    let family = op.provider_family();
    let known = |id: &str| cp.alias_to_route.contains_key(id) || cp.routes_by_name.contains_key(id);

    let body = match op.operation {
        Operation::ListModels => {
            let mut ids: Vec<&String> = cp
                .alias_to_route
                .keys()
                .chain(cp.routes_by_name.keys())
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
            if !known(&id) {
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

    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    Ok(ExecOutcome {
        status: http::StatusCode::OK,
        headers,
        body: ResponseBody::Full(body),
        disposition: crate::channel::disposition::Disposition::Success,
    })
}
