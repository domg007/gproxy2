use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::affinity::{CacheAffinityHint, CacheAffinityPool};
use crate::channel::Channel;
use crate::health::CredentialHealth;
use crate::request::PreparedRequest;
use crate::response::{ResponseClassification, UpstreamError, UpstreamResponse};
use tracing::Instrument;

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
    retry_with_credentials_inner(ctx, send)
        .instrument(span)
        .await
}

async fn retry_with_credentials_inner<C, F, Fut>(
    ctx: RetryContext<'_, C>,
    send: F,
) -> Result<UpstreamResponse, UpstreamError>
where
    C: Channel,
    F: Fn(&wreq::Client, http::Request<Vec<u8>>) -> Fut,
    Fut: std::future::Future<Output = Result<UpstreamResponse, UpstreamError>>,
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

    let mut remaining = build_remaining_candidates(&eligible, round_robin_cursor);
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
            let response = match send(active_client, http_request).await {
                Ok(resp) => resp,
                Err(e) => {
                    tracing::warn!(credential = idx, %method, %uri, error = %e, "upstream request failed");
                    last_error = Some(e);
                    break;
                }
            };

            // Classify response
            let latency_ms = send_start.elapsed().as_millis() as u64;
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
                    if let Some(hint) = affinity_hint {
                        affinity_pool.bind(&hint.bind.key, idx, hint.bind.ttl_ms);
                        if let Some(matched_idx) = matched_affinity_idx
                            && let Some(hit) = hint.candidates.get(matched_idx)
                        {
                            affinity_pool.bind(&hit.key, idx, hit.ttl_ms);
                        }
                    }
                    return Ok(response);
                }
                ResponseClassification::AuthDead => {
                    tracing::warn!(
                        credential = idx,
                        status = response.status,
                        model = model.unwrap_or(""),
                        "credential auth dead, attempting refresh"
                    );
                    // Try refreshing the credential (OAuth token exchange, etc.)
                    let (credential, health) = &mut credentials[idx];
                    let refreshed = channel
                        .refresh_credential(http_client, credential)
                        .await
                        .unwrap_or(false);

                    if refreshed {
                        // Retry once with the refreshed credential
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
                            Ok(retry_response) => {
                                let retry_class = channel.classify_response(
                                    retry_response.status,
                                    &retry_response.headers,
                                    &retry_response.body,
                                );
                                if matches!(retry_class, ResponseClassification::Success) {
                                    health.record_success(model);
                                    if let Some(hint) = affinity_hint {
                                        affinity_pool.bind(&hint.bind.key, idx, hint.bind.ttl_ms);
                                        if let Some(matched_idx) = matched_affinity_idx
                                            && let Some(hit) = hint.candidates.get(matched_idx)
                                        {
                                            affinity_pool.bind(&hit.key, idx, hit.ttl_ms);
                                        }
                                    }
                                    return Ok(retry_response);
                                }
                                // Still failing after refresh — mark dead
                                health.record_error(retry_response.status, model, None);
                                tracing::warn!(
                                    credential = idx,
                                    status = retry_response.status,
                                    "credential still dead after refresh"
                                );
                            }
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
                    if let Some(matched_idx) = matched_affinity_idx
                        && let Some(hint) = affinity_hint
                        && let Some(hit) = hint.candidates.get(matched_idx)
                    {
                        affinity_pool.clear(&hit.key);
                    }
                    break;
                }
                ResponseClassification::RateLimited { retry_after_ms } => {
                    if retry_after_ms.is_some() {
                        // Has retry-after: cooldown this credential, move to next
                        health.record_error(response.status, model, retry_after_ms);
                        tracing::warn!(
                            credential = idx,
                            status = response.status,
                            retry_after_ms = retry_after_ms.unwrap_or(0),
                            model = model.unwrap_or(""),
                            "rate limited with retry-after, switching credential"
                        );
                        if let Some(matched_idx) = matched_affinity_idx
                            && let Some(hint) = affinity_hint
                            && let Some(hit) = hint.candidates.get(matched_idx)
                        {
                            affinity_pool.clear(&hit.key);
                        }
                        break;
                    }
                    // No retry-after: retry same credential up to max_retries
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
                        if let Some(matched_idx) = matched_affinity_idx
                            && let Some(hint) = affinity_hint
                            && let Some(hit) = hint.candidates.get(matched_idx)
                        {
                            affinity_pool.clear(&hit.key);
                        }
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
                    if let Some(matched_idx) = matched_affinity_idx
                        && let Some(hint) = affinity_hint
                        && let Some(hit) = hint.candidates.get(matched_idx)
                    {
                        affinity_pool.clear(&hit.key);
                    }
                    break;
                }
                ResponseClassification::PermanentError => {
                    return Ok(response);
                }
            }
        }
    }

    Err(last_error.unwrap_or(UpstreamError::AllCredentialsExhausted))
}

fn build_remaining_candidates(eligible: &[usize], round_robin_cursor: &AtomicUsize) -> Vec<usize> {
    if eligible.is_empty() {
        return Vec::new();
    }

    let start = round_robin_cursor.fetch_add(1, Ordering::Relaxed) % eligible.len();
    (0..eligible.len())
        .map(|offset| eligible[(start + offset) % eligible.len()])
        .collect()
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
