//! Upstream failover loop (§6.4) — the ONE place candidates are iterated and
//! `Channel::classify` is called. Stream & non-stream share the loop and differ
//! only at the body tail (D4). M2: the transform plan (passthrough vs
//! cross-protocol) is resolved PER candidate, before `prepare`.

use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};

use crate::app::AppState;
use crate::channel::{Channel, PrepareCtx};
use crate::http::client::{ClientError, UpstreamClient};
use crate::pipeline::context::{Candidate, RequestCtx};
use crate::pipeline::error::PipelineError;
use crate::pipeline::outcome::{ExecOutcome, ResponseBody};
use crate::pipeline::transform::{self as transform_step, AttemptMemo, TransformPlan};

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
        let (plan, rules) = {
            let cp = state.cp();
            let plan = match transform_step::plan_for(
                &cp,
                cand.provider.id,
                ctx.op.expect("classified"),
                channel.target_kind(),
            ) {
                Ok(p) => p,
                // `local` is intentional config addressed to this request —
                // surface it, don't shop for another candidate.
                Err(e @ PipelineError::LocalUnimplemented) => return Err(e),
                // unsupported / no-pair: this candidate can't serve it; next.
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            };
            let rules = cp.rule_sets_by_provider.get(&cand.provider.id).cloned();
            (plan, rules)
        };

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
            method: ctx.method.clone(),
            path: &parts.path,
            query: parts.query.as_deref(),
            headers: parts.headers.as_ref().unwrap_or(&ctx.headers),
            body: parts.body,
        }) {
            Ok(p) => p,
            Err(e) => {
                last_err = Some(PipelineError::Channel(e));
                continue;
            }
        };

        let (status, mut headers, source) =
            match send_once(state.upstream.as_ref(), prepared.into_http(), ctx.stream).await {
                Ok(t) => t,
                Err(e) => {
                    last_err = Some(PipelineError::Transport(e.to_string()));
                    continue;
                }
            };

        let disposition = match &source {
            BodySource::Buffered(b) => channel.classify(status, &headers, b),
            #[cfg(not(target_arch = "wasm32"))]
            BodySource::Streaming(_) => channel.classify(status, &headers, &Bytes::new()),
        };

        if !disposition.should_failover() {
            // Success, or a Permanent 4xx the client should see — return it.
            let body = materialize(&channel, source, &plan, ctx, status)?;
            if plan.is_transform() {
                // converted bytes no longer match the upstream framing
                headers.remove(http::header::CONTENT_LENGTH);
            }
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
