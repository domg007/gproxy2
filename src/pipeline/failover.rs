//! Upstream failover loop (§6.4) — the ONE place candidates are iterated and
//! `Channel::classify` is called. Stream & non-stream share the loop and differ
//! only at the body tail (D4).

use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};

use crate::app::AppState;
use crate::channel::{Channel, PrepareCtx};
use crate::http::client::{ClientError, UpstreamClient};
use crate::pipeline::context::{Candidate, RequestCtx};
use crate::pipeline::error::PipelineError;
use crate::pipeline::outcome::{ExecOutcome, ResponseBody};

/// Uniform per-attempt response body source. `Streaming` is native-only; on wasm
/// the executor always buffers, so `classify` runs identically on status+headers
/// regardless of mode and only the tail materialization branches.
pub enum BodySource {
    Buffered(Bytes),
    #[cfg(not(target_arch = "wasm32"))]
    Streaming(crate::http::client::RespStream),
}

/// Iterate candidates until one succeeds or returns a permanent error. The
/// channel is resolved PER candidate (a route's members may span providers /
/// channels).
pub async fn run_failover(
    state: &AppState,
    ctx: &RequestCtx,
    candidates: &[Candidate],
) -> Result<ExecOutcome, PipelineError> {
    let mut last_err: Option<PipelineError> = None;

    for cand in candidates {
        let Some(channel) = state.channels.get(&cand.provider.channel) else {
            last_err = Some(PipelineError::UnknownChannel(cand.provider.channel.clone()));
            continue;
        };

        let prepared = match channel.prepare(PrepareCtx {
            secret: &cand.credential.secret_json,
            provider_settings: &cand.provider.settings_json,
            upstream_model_id: &cand.upstream_model_id,
            method: ctx.method.clone(),
            path: &ctx.path,
            query: ctx.query.as_deref(),
            headers: &ctx.headers,
            body: ctx.body.clone(),
        }) {
            Ok(p) => p,
            Err(e) => {
                last_err = Some(PipelineError::Channel(e));
                continue;
            }
        };

        let (status, headers, source) =
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
            let body = materialize(&channel, source);
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

/// Materialize an attempt's body into the executor response body.
fn materialize(channel: &Arc<dyn Channel>, source: BodySource) -> ResponseBody {
    match source {
        BodySource::Buffered(b) => ResponseBody::Full(channel.normalize(b)),
        #[cfg(not(target_arch = "wasm32"))]
        BodySource::Streaming(st) => {
            ResponseBody::Stream(crate::pipeline::stream::into_byte_stream(st))
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
