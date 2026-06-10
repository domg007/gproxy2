//! Upstream failover loop (§6.4) — the ONE place candidates are iterated and
//! `Channel::classify` is called. Stream & non-stream share the loop and differ
//! only at the body tail (D4). M2: the transform plan (passthrough vs
//! cross-protocol) is resolved PER candidate, before `prepare`.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};

use crate::app::AppState;
use crate::channel::{Channel, PrepareCtx};
use crate::http::client::{ClientError, UpstreamClient};
use crate::pipeline::context::{Candidate, RequestCtx};
use crate::pipeline::error::PipelineError;
use crate::pipeline::health_hooks;
use crate::pipeline::local_ops;
use crate::pipeline::outcome::{ExecOutcome, ResponseBody};
use crate::pipeline::transform::{self as transform_step, AttemptMemo, TransformPlan};
use crate::protocol::Operation;

/// Uniform per-attempt response body source. `Streaming` is native-only; on wasm
/// the executor always buffers, so `classify` runs identically on status+headers
/// regardless of mode and only the tail materialization branches.
pub enum BodySource {
    Buffered(Bytes),
    #[cfg(not(target_arch = "wasm32"))]
    Streaming(crate::http::client::RespStream),
}

/// Iterate candidates until one succeeds or returns a permanent error. The
/// channel AND the transform plan are resolved PER candidate (a route's members
/// may span providers / channels / wire protocols).
pub async fn run_failover(
    state: &AppState,
    ctx: &RequestCtx,
    candidates: &[Candidate],
) -> Result<ExecOutcome, PipelineError> {
    let mut last_err: Option<PipelineError> = None;
    let mut memo = AttemptMemo::default();

    for cand in candidates {
        let Some(channel) = state.channels.get(&cand.provider.channel) else {
            last_err = Some(PipelineError::UnknownChannel(cand.provider.channel.clone()));
            continue;
        };

        // M2 dispatch decision per candidate. The snapshot guard is scoped to
        // this lookup only — never held across the upstream call.
        let (plan, rules, local_models) = {
            let cp = state.cp();
            let plan = match transform_step::plan_for(
                &cp,
                cand.provider.id,
                ctx.op.expect("classified"),
                channel.target_kind(),
            ) {
                // `local` is intentional config addressed to this request —
                // serve it here, never shop for another candidate.
                Ok(TransformPlan::Local) => {
                    return match local_ops::serve_local(state, &cp, ctx, cand) {
                        Some(o) => Ok(o),
                        None => Err(PipelineError::LocalUnimplemented),
                    };
                }
                Ok(p) => p,
                // unsupported / no-pair: this candidate can't serve it; next.
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            };
            let rules = cp.rule_sets_by_provider.get(&cand.provider.id).cloned();
            // §6.3 merged models: manual + variant rows join a successful
            // upstream list (additions captured while the guard is held).
            let local_models = (ctx.op.expect("classified").operation == Operation::ListModels)
                .then(|| {
                    cp.exposed_models_by_provider
                        .get(&cand.provider.id)
                        .cloned()
                })
                .flatten();
            (plan, rules, local_models)
        };

        // §3.3 per-credential rpm/tpm budget — a budget skip is not a health
        // failure (the key is fine, just busy this minute).
        if budget_exhausted(state, cand).await {
            last_err = Some(PipelineError::Transport(
                "credential rpm budget exhausted".into(),
            ));
            continue;
        }

        let parts = match transform_step::request_parts(
            ctx,
            cand,
            &plan,
            rules.as_deref().map(|v| v.as_slice()),
            &mut memo,
        ) {
            Ok(p) => p,
            Err(e) => {
                last_err = Some(e);
                continue;
            }
        };

        let prepared = match channel.prepare(PrepareCtx {
            secret: &cand.credential.secret_json,
            provider_settings: &cand.provider.settings_json,
            upstream_model_id: &cand.upstream_model_id,
            method: parts.method.clone(),
            path: &parts.path,
            query: parts.query.as_deref(),
            headers: parts.headers.as_ref().unwrap_or(&ctx.headers),
            body: parts.body,
        }) {
            Ok(p) => p,
            Err(e) => {
                // Prepare failures count against health like transient errors.
                health_hooks::record_failure(state, cand);
                last_err = Some(PipelineError::Channel(e));
                continue;
            }
        };

        // §7.4 effective proxy per attempt → per-proxy client; wasm and
        // non-wreq builds always use the default upstream client.
        #[cfg(all(not(target_arch = "wasm32"), feature = "upstream-wreq"))]
        let client = state.client_pool.for_proxy(
            crate::channel::resolve::effective_proxy(
                &cand.credential,
                &cand.provider,
                state.config.upstream.proxy_url.as_deref(),
            )
            .as_deref(),
        );
        #[cfg(not(all(not(target_arch = "wasm32"), feature = "upstream-wreq")))]
        let client = Arc::clone(&state.upstream);

        #[cfg(not(target_arch = "wasm32"))]
        let send_started = std::time::Instant::now();

        let (status, mut headers, source) =
            match send_once(client.as_ref(), prepared.into_http(), ctx.stream).await {
                Ok(t) => t,
                Err(e) => {
                    health_hooks::record_failure(state, cand);
                    last_err = Some(PipelineError::Transport(e.to_string()));
                    continue;
                }
            };

        // Send latency feeds the member EWMA (native only; wasm has no
        // monotonic clock worth trusting here).
        #[cfg(not(target_arch = "wasm32"))]
        let send_ms = Some(send_started.elapsed().as_secs_f64() * 1000.0);
        #[cfg(target_arch = "wasm32")]
        let send_ms = None;

        let disposition = match &source {
            BodySource::Buffered(b) => channel.classify(status, &headers, b),
            #[cfg(not(target_arch = "wasm32"))]
            BodySource::Streaming(_) => channel.classify(status, &headers, &Bytes::new()),
        };

        // §3.2/§16.3 disposition → health (+ edge-persisted credential edges).
        health_hooks::record_attempt(state, cand, &disposition, send_ms);

        if !disposition.should_failover() {
            // Success, or a Permanent 4xx the client should see — return it.
            let body = materialize(&channel, source, &plan, ctx, status)?;
            if plan.is_transform() {
                // converted bytes no longer match the upstream framing
                headers.remove(http::header::CONTENT_LENGTH);
            }
            // §6.3 merged models: append manual + variant entries to a
            // successful upstream list (inbound-shaped by now).
            let body = match (&local_models, body) {
                (Some(models), ResponseBody::Full(b)) if status.is_success() => {
                    headers.remove(http::header::CONTENT_LENGTH);
                    let family = ctx.op.expect("classified").provider_family();
                    ResponseBody::Full(local_ops::merge_into_list(
                        family,
                        b,
                        &local_ops::entries_from(models),
                    ))
                }
                (_, body) => body,
            };
            return Ok(ExecOutcome {
                status,
                headers,
                body,
                disposition,
            });
        }

        // AuthDead / RateLimited / Transient → drop this attempt, try the next.
        last_err = Some(PipelineError::Transport(format!(
            "upstream {status} ({disposition:?})"
        )));
    }

    // §6.3 count fallback: count must not fail just because upstreams did —
    // answer locally (the first candidate supplies provider settings).
    if ctx.op.expect("classified").operation == Operation::CountTokens
        && let Some(cand) = candidates.first()
        && let Some(o) = local_ops::serve_local(state, &state.cp(), ctx, cand)
    {
        tracing::warn!("all upstream count attempts failed; serving local count fallback");
        return Ok(o);
    }

    Err(last_err.unwrap_or(PipelineError::AllAttemptsFailed))
}

/// Materialize an attempt's body. Response-direction transform applies only to
/// 2xx bodies — error payloads stay provider-native (M2 fidelity note).
fn materialize(
    channel: &Arc<dyn Channel>,
    source: BodySource,
    plan: &TransformPlan,
    ctx: &RequestCtx,
    status: StatusCode,
) -> Result<ResponseBody, PipelineError> {
    match source {
        BodySource::Buffered(b) => {
            let b = channel.normalize(b);
            if !status.is_success() || !plan.is_transform() {
                return Ok(ResponseBody::Full(b));
            }
            if ctx.stream {
                // buffered streaming (wasm): convert the whole SSE body
                let t = transform_step::stream_transformer(plan).expect("transform plan");
                Ok(ResponseBody::Full(Bytes::from(
                    crate::transform::stream_adapter::convert_buffered(t, &b),
                )))
            } else {
                Ok(ResponseBody::Full(transform_step::response_body(plan, b)?))
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        BodySource::Streaming(st) => {
            if !status.is_success() {
                return Ok(ResponseBody::Stream(
                    crate::pipeline::stream::into_byte_stream(st),
                ));
            }
            match transform_step::stream_transformer(plan) {
                None => Ok(ResponseBody::Stream(
                    crate::pipeline::stream::into_byte_stream(st),
                )),
                Some(t) => Ok(ResponseBody::Stream(
                    crate::pipeline::stream::transform_byte_stream(st, t),
                )),
            }
        }
    }
}

/// Per-credential minute budgets (§3.3), incr-then-check on the shared cache
/// (same off-by-one semantics as authz). rpm increments per attempt; tpm is a
/// read-only seam — nothing increments `ctpm:*` until M6 feeds real usage.
async fn budget_exhausted(state: &AppState, cand: &Candidate) -> bool {
    let bucket = crate::util::time::unix_now() / 60;
    let ttl = Some(Duration::from_secs(120));
    if let Some(limit) = cand.credential.rpm_limit {
        let key = format!("crpm:{}:m{bucket}", cand.credential.id);
        if state.cache.incr(&key, 1, ttl).await > limit {
            return true;
        }
    }
    if let Some(limit) = cand.credential.tpm_limit {
        let key = format!("ctpm:{}:m{bucket}", cand.credential.id);
        if state.cache.incr(&key, 0, ttl).await > limit {
            return true;
        }
    }
    false
}

/// One upstream send → uniform `(status, headers, BodySource)`. Streaming on
/// native when requested; always buffered on wasm.
#[cfg(not(target_arch = "wasm32"))]
async fn send_once(
    client: &dyn UpstreamClient,
    req: http::Request<Bytes>,
    stream: bool,
) -> Result<(StatusCode, HeaderMap, BodySource), ClientError> {
    if stream {
        let (status, headers, st) = client.send_streaming(req).await?;
        Ok((status, headers, BodySource::Streaming(st)))
    } else {
        let resp = client.send(req).await?;
        let (parts, body) = resp.into_parts();
        Ok((parts.status, parts.headers, BodySource::Buffered(body)))
    }
}

#[cfg(target_arch = "wasm32")]
async fn send_once(
    client: &dyn UpstreamClient,
    req: http::Request<Bytes>,
    _stream: bool,
) -> Result<(StatusCode, HeaderMap, BodySource), ClientError> {
    let resp = client.send(req).await?;
    let (parts, body) = resp.into_parts();
    Ok((parts.status, parts.headers, BodySource::Buffered(body)))
}
