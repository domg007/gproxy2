use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::affinity::{CacheAffinityHint, CacheAffinityPool};
use crate::channel::Channel;
use crate::health::CredentialHealth;
use crate::request::PreparedRequest;
use crate::response::{
    ResponseClassification, RetryableUpstreamResponse, UpstreamError, UpstreamResponse,
    UpstreamStreamingResponse,
};
use tracing::Instrument;

// ---------------------------------------------------------------------------
// RetryableResult trait — abstracts buffered vs streaming response handling
// ---------------------------------------------------------------------------

/// Action determined after inspecting a raw upstream response.
enum RetryAction<T> {
    /// 2xx streaming — return immediately, body cannot be inspected.
    ImmediateSuccess { status: u16, output: T },
    /// Body is buffered and can be classified for retry decisions.
    Classifiable(UpstreamResponse),
}

/// Abstracts over buffered (`UpstreamResponse`) and streaming
/// (`RetryableUpstreamResponse`) so the retry loop can be written once.
trait RetryableResult: Sized {
    /// The caller's final success type.
    type Output;

    /// Inspect the raw response and decide whether it's an immediate success
    /// (streaming 2xx) or needs classification.
    fn into_retry_action(self) -> RetryAction<Self::Output>;

    /// Wrap a fully-buffered response into the caller's output type.
    /// Used for Success and PermanentError paths after classification.
    fn wrap_buffered(response: UpstreamResponse) -> Self::Output;
}

impl RetryableResult for UpstreamResponse {
    type Output = UpstreamResponse;

    fn into_retry_action(self) -> RetryAction<Self::Output> {
        RetryAction::Classifiable(self)
    }

    fn wrap_buffered(response: UpstreamResponse) -> Self::Output {
        response
    }
}

impl RetryableResult for RetryableUpstreamResponse {
    type Output = UpstreamStreamingResponse;

    fn into_retry_action(self) -> RetryAction<Self::Output> {
        match self {
            RetryableUpstreamResponse::Streaming(s) => RetryAction::ImmediateSuccess {
                status: s.status,
                output: s,
            },
            RetryableUpstreamResponse::Buffered(b) => RetryAction::Classifiable(b),
        }
    }

    fn wrap_buffered(response: UpstreamResponse) -> Self::Output {
        UpstreamStreamingResponse {
            status: response.status,
            headers: response.headers,
            body: Box::pin(futures_util::stream::once(async move {
                Ok(bytes::Bytes::from(response.body))
            })),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parameters for credential-rotating retry.
pub struct RetryContext<'a, C: Channel> {
    pub channel: &'a C,
    pub credentials: &'a mut [(C::Credential, C::Health)],
    pub settings: &'a C::Settings,
    pub request: &'a PreparedRequest,
    pub affinity_hint: Option<&'a CacheAffinityHint>,
    pub affinity_pool: &'a CacheAffinityPool,
    pub round_robin_cursor: &'a AtomicUsize,
    pub max_retries: u32,
    pub http_client: &'a wreq::Client,
    /// Browser-impersonating client for credentials that need cookie auth.
    /// Falls back to `http_client` when `None`.
    pub spoof_client: Option<&'a wreq::Client>,
}

/// Retry a request across multiple credentials.
///
/// For each eligible credential, tries up to `max_retries` times on 429
/// without `retry-after`. If 429 includes `retry-after`, the credential
/// is marked with a cooldown and skipped immediately.
///
/// On 401/403 (AuthDead), calls `channel.refresh_credential` to attempt
/// a token refresh. If refresh succeeds, retries once. If the retry also
/// fails with AuthDead, the credential is marked dead.
///
/// The caller provides a `send` closure that performs the actual HTTP request.
pub async fn retry_with_credentials<C, F, Fut>(
    ctx: RetryContext<'_, C>,
    send: F,
) -> Result<UpstreamResponse, UpstreamError>
where
    C: Channel,
    F: Fn(&wreq::Client, http::Request<Vec<u8>>) -> Fut,
    Fut: std::future::Future<Output = Result<UpstreamResponse, UpstreamError>>,
{
    let span = tracing::info_span!(
        "retry_with_credentials",
        model = ctx.request.model.as_deref().unwrap_or(""),
        credentials = ctx.credentials.len(),
        max_retries = ctx.max_retries,
    );
    retry_common_inner(ctx, send).instrument(span).await
}

/// Retry a request across multiple credentials while preserving successful
/// upstream bodies as a stream.
pub async fn retry_with_credentials_stream<C, F, Fut>(
    ctx: RetryContext<'_, C>,
    send: F,
) -> Result<UpstreamStreamingResponse, UpstreamError>
where
    C: Channel,
    F: Fn(&wreq::Client, http::Request<Vec<u8>>) -> Fut,
    Fut: std::future::Future<Output = Result<RetryableUpstreamResponse, UpstreamError>>,
{
    let span = tracing::info_span!(
        "retry_with_credentials_stream",
        model = ctx.request.model.as_deref().unwrap_or(""),
        credentials = ctx.credentials.len(),
        max_retries = ctx.max_retries,
    );
    retry_common_inner(ctx, send).instrument(span).await
}

// ---------------------------------------------------------------------------
// Unified retry loop
// ---------------------------------------------------------------------------

async fn retry_common_inner<C, F, Fut, R>(
    ctx: RetryContext<'_, C>,
    send: F,
) -> Result<R::Output, UpstreamError>
where
    C: Channel,
    F: Fn(&wreq::Client, http::Request<Vec<u8>>) -> Fut,
    Fut: std::future::Future<Output = Result<R, UpstreamError>>,
    R: RetryableResult,
{
    let RetryContext {
        channel,
        credentials,
        settings,
        request,
        affinity_hint,
        affinity_pool,
        round_robin_cursor,
        max_retries,
        http_client,
        spoof_client,
    } = ctx;

    let model = request.model.as_deref();

    // Filter to eligible credentials
    let eligible: Vec<usize> = credentials
        .iter()
        .enumerate()
        .filter(|(_, (_, health))| health.is_available(model))
        .map(|(i, _)| i)
        .collect();

    if eligible.is_empty() {
        return Err(UpstreamError::NoEligibleCredentials);
    }

    let mut remaining = build_remaining_candidates(
        &eligible,
        round_robin_cursor,
        affinity_hint.is_some(),
    );
    let mut last_error = None;

    while !remaining.is_empty() {
        let (remaining_idx, matched_affinity_idx) =
            pick_candidate_index(&remaining, affinity_hint, affinity_pool);
        let idx = remaining.remove(remaining_idx);
        tracing::info!(credential = idx, "trying credential");
        let mut attempts = 0u32;

        loop {
            let (credential, _) = &credentials[idx];

            // Select client: spoof for cookie-based credentials, normal otherwise
            let active_client = if channel.needs_spoof_client(credential) {
                spoof_client.unwrap_or(http_client)
            } else {
                http_client
            };

            // Build HTTP request
            let http_request = match channel.prepare_request(credential, settings, request) {
                Ok(req) => req,
                Err(e) => {
                    tracing::warn!(credential = idx, error = %e, "failed to prepare request");
                    last_error = Some(e);
                    break;
                }
            };

            // Send request
            let method = http_request.method().as_str().to_string();
            let uri = http_request.uri().to_string();
            tracing::info!(
                credential = idx,
                attempt = attempts,
                %method,
                %uri,
                model = model.unwrap_or(""),
                "sending upstream request"
            );
            let send_start = std::time::Instant::now();
            let raw_response = match send(active_client, http_request).await {
                Ok(resp) => resp,
                Err(e) => {
                    tracing::warn!(credential = idx, %method, %uri, error = %e, "upstream request failed");
                    last_error = Some(e);
                    break;
                }
            };

            let latency_ms = send_start.elapsed().as_millis() as u64;

            // Determine if this is an immediate success (streaming 2xx) or needs classification
            let response = match raw_response.into_retry_action() {
                RetryAction::ImmediateSuccess { status, output } => {
                    tracing::info!(
                        credential = idx,
                        status,
                        latency_ms,
                        "upstream response received"
                    );
                    let (_, health) = &mut credentials[idx];
                    health.record_success(model);
                    bind_affinity(affinity_pool, affinity_hint, idx, matched_affinity_idx);
                    return Ok(output);
                }
                RetryAction::Classifiable(resp) => resp,
            };

            // Classify buffered response
            tracing::info!(
                credential = idx,
                status = response.status,
                latency_ms,
                "upstream response received"
            );
            let classification =
                channel.classify_response(response.status, &response.headers, &response.body);

            let (_, health) = &mut credentials[idx];
            match classification {
                ResponseClassification::Success => {
                    health.record_success(model);
                    bind_affinity(affinity_pool, affinity_hint, idx, matched_affinity_idx);
                    return Ok(R::wrap_buffered(response));
                }
                ResponseClassification::AuthDead => {
                    tracing::warn!(
                        credential = idx,
                        status = response.status,
                        model = model.unwrap_or(""),
                        "credential auth dead, attempting refresh"
                    );
                    let (credential, health) = &mut credentials[idx];
                    let refreshed = channel
                        .refresh_credential(http_client, credential)
                        .await
                        .unwrap_or(false);

                    if refreshed {
                        let retry_request = match channel
                            .prepare_request(credential, settings, request)
                        {
                            Ok(req) => req,
                            Err(e) => {
                                tracing::warn!(credential = idx, error = %e, "failed to prepare request after refresh");
                                last_error = Some(e);
                                break;
                            }
                        };

                        match send(active_client, retry_request).await {
                            Ok(raw_retry) => match raw_retry.into_retry_action() {
                                RetryAction::ImmediateSuccess { output, .. } => {
                                    health.record_success(model);
                                    bind_affinity(
                                        affinity_pool,
                                        affinity_hint,
                                        idx,
                                        matched_affinity_idx,
                                    );
                                    return Ok(output);
                                }
                                RetryAction::Classifiable(retry_response) => {
                                    let retry_class = channel.classify_response(
                                        retry_response.status,
                                        &retry_response.headers,
                                        &retry_response.body,
                                    );
                                    if matches!(retry_class, ResponseClassification::Success) {
                                        health.record_success(model);
                                        bind_affinity(
                                            affinity_pool,
                                            affinity_hint,
                                            idx,
                                            matched_affinity_idx,
                                        );
                                        return Ok(R::wrap_buffered(retry_response));
                                    }
                                    health.record_error(retry_response.status, model, None);
                                    tracing::warn!(
                                        credential = idx,
                                        status = retry_response.status,
                                        "credential still dead after refresh"
                                    );
                                }
                            },
                            Err(e) => {
                                tracing::warn!(credential = idx, error = %e, "upstream request failed after refresh");
                                last_error = Some(e);
                            }
                        }
                    } else {
                        health.record_error(response.status, model, None);
                        tracing::warn!(
                            credential = idx,
                            status = response.status,
                            "credential auth dead, refresh not available"
                        );
                    }
                    clear_affinity(affinity_pool, affinity_hint, matched_affinity_idx);
                    break;
                }
                ResponseClassification::RateLimited { retry_after_ms } => {
                    if retry_after_ms.is_some() {
                        health.record_error(response.status, model, retry_after_ms);
                        tracing::warn!(
                            credential = idx,
                            status = response.status,
                            retry_after_ms = retry_after_ms.unwrap_or(0),
                            model = model.unwrap_or(""),
                            "rate limited with retry-after, switching credential"
                        );
                        clear_affinity(affinity_pool, affinity_hint, matched_affinity_idx);
                        break;
                    }
                    attempts += 1;
                    if attempts >= max_retries {
                        health.record_error(response.status, model, None);
                        tracing::warn!(
                            credential = idx,
                            status = response.status,
                            attempts,
                            max_retries,
                            model = model.unwrap_or(""),
                            "rate limited, retries exhausted"
                        );
                        clear_affinity(affinity_pool, affinity_hint, matched_affinity_idx);
                        break;
                    }
                    tracing::info!(
                        credential = idx,
                        status = response.status,
                        attempt = attempts,
                        max_retries,
                        "rate limited without retry-after, retrying"
                    );
                    continue;
                }
                ResponseClassification::TransientError => {
                    health.record_error(response.status, model, None);
                    tracing::warn!(
                        credential = idx,
                        status = response.status,
                        model = model.unwrap_or(""),
                        "transient error"
                    );
                    clear_affinity(affinity_pool, affinity_hint, matched_affinity_idx);
                    break;
                }
                ResponseClassification::PermanentError => {
                    return Ok(R::wrap_buffered(response));
                }
            }
        }
    }

    Err(last_error.unwrap_or(UpstreamError::AllCredentialsExhausted))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn bind_affinity(
    pool: &CacheAffinityPool,
    hint: Option<&CacheAffinityHint>,
    idx: usize,
    matched_affinity_idx: Option<usize>,
) {
    if let Some(hint) = hint {
        pool.bind(&hint.bind.key, idx, hint.bind.ttl_ms);
        if let Some(matched_idx) = matched_affinity_idx
            && let Some(hit) = hint.candidates.get(matched_idx)
        {
            pool.bind(&hit.key, idx, hit.ttl_ms);
        }
    }
}

fn clear_affinity(
    pool: &CacheAffinityPool,
    hint: Option<&CacheAffinityHint>,
    matched_affinity_idx: Option<usize>,
) {
    if let Some(matched_idx) = matched_affinity_idx
        && let Some(hint) = hint
        && let Some(hit) = hint.candidates.get(matched_idx)
    {
        pool.clear(&hit.key);
    }
}

fn build_remaining_candidates(
    eligible: &[usize],
    round_robin_cursor: &AtomicUsize,
    use_random: bool,
) -> Vec<usize> {
    if eligible.is_empty() {
        return Vec::new();
    }

    if use_random {
        // Random order: cache affinity will steer to the right credential,
        // and random base order prevents sequential bias that undermines affinity.
        use rand::seq::SliceRandom;
        let mut candidates: Vec<usize> = eligible.to_vec();
        candidates.shuffle(&mut rand::rng());
        candidates
    } else {
        // Round-robin: deterministic rotation across credentials.
        let start = round_robin_cursor.fetch_add(1, Ordering::Relaxed) % eligible.len();
        (0..eligible.len())
            .map(|offset| eligible[(start + offset) % eligible.len()])
            .collect()
    }
}

fn pick_candidate_index(
    remaining: &[usize],
    affinity_hint: Option<&CacheAffinityHint>,
    affinity_pool: &CacheAffinityPool,
) -> (usize, Option<usize>) {
    let Some(hint) = affinity_hint else {
        return (0, None);
    };

    let remaining_idx_by_credential = remaining
        .iter()
        .enumerate()
        .map(|(idx, credential_idx)| (*credential_idx, idx))
        .collect::<HashMap<_, _>>();
    let mut score_by_credential = HashMap::<usize, usize>::new();
    let mut representative_match = HashMap::<usize, (usize, usize)>::new();

    for (candidate_idx, candidate) in hint.candidates.iter().enumerate() {
        let Some(credential_idx) = affinity_pool.get(&candidate.key) else {
            continue;
        };
        if !remaining_idx_by_credential.contains_key(&credential_idx) {
            continue;
        }

        let score = score_by_credential.entry(credential_idx).or_default();
        *score = score.saturating_add(candidate.key_len);

        representative_match
            .entry(credential_idx)
            .and_modify(|(best_idx, best_len)| {
                if candidate.key_len > *best_len {
                    *best_idx = candidate_idx;
                    *best_len = candidate.key_len;
                }
            })
            .or_insert((candidate_idx, candidate.key_len));
    }

    let mut best: Option<(usize, usize, usize)> = None;
    for (credential_idx, score) in score_by_credential {
        let Some(&remaining_idx) = remaining_idx_by_credential.get(&credential_idx) else {
            continue;
        };
        let matched_idx = representative_match
            .get(&credential_idx)
            .map(|(idx, _)| *idx)
            .unwrap_or_default();

        match best {
            None => best = Some((remaining_idx, score, matched_idx)),
            Some((best_remaining_idx, best_score, _)) => {
                if score > best_score || (score == best_score && remaining_idx < best_remaining_idx)
                {
                    best = Some((remaining_idx, score, matched_idx));
                }
            }
        }
    }

    if let Some((remaining_idx, _, matched_idx)) = best {
        (remaining_idx, Some(matched_idx))
    } else {
        (0, None)
    }
}
