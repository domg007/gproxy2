use super::*;

pub async fn retry_with_eligible_credentials_with_affinity<Material, T, Select, Attempt, Fut>(
    context: CredentialRetryContext<'_>,
    mut select_material: Select,
    mut attempt: Attempt,
) -> Result<T, UpstreamError>
where
    Select: FnMut(&CredentialRef) -> Option<Material>,
    Attempt: FnMut(CredentialAttempt<Material>) -> Fut,
    Fut: Future<Output = CredentialRetryDecision<T>>,
{
    let CredentialRetryContext {
        provider,
        credential_states,
        model,
        now_unix_ms,
        pick_mode,
        cache_affinity_hint,
    } = context;
    let normalized_model = model.and_then(normalize_model_cooldown_key);
    let mut remaining = credential_states
        .eligible_credentials(
            &provider.channel,
            provider.credentials.list_credentials(),
            normalized_model.as_deref(),
            now_unix_ms,
        )
        .into_iter()
        .filter_map(|credential| {
            select_material(credential).map(|material| CredentialCandidate {
                credential_id: credential.id,
                material,
            })
        })
        .collect::<Vec<_>>();

    if remaining.is_empty() {
        return Err(UpstreamError::NoEligibleCredential {
            channel: provider.channel.as_str().to_string(),
            model: normalized_model,
        });
    }

    let use_cache_affinity = matches!(pick_mode, CredentialPickMode::RoundRobinWithCache);
    let scoped_candidates = if use_cache_affinity {
        cache_affinity_hint
            .as_ref()
            .map(|hint| scoped_affinity_candidates(provider, hint))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let scoped_bind = if use_cache_affinity {
        cache_affinity_hint
            .as_ref()
            .map(|hint| ScopedAffinityCandidate {
                scoped_key: scoped_affinity_key(provider, hint.bind.key.as_str()),
                ttl_ms: hint.bind.ttl_ms,
                key_len: hint.bind.key_len,
            })
    } else {
        None
    };

    if affinity_trace_enabled() {
        let model_for_log = normalized_model.as_deref().unwrap_or("-");
        let candidate_preview = scoped_candidates
            .iter()
            .take(3)
            .map(|item| item.scoped_key.as_str())
            .collect::<Vec<_>>();
        tracing::info!(
            channel=%provider.channel.as_str(),
            model=%model_for_log,
            configured_pick_mode=?provider.credential_pick_mode,
            cache_affinity_max_keys=provider.cache_affinity_max_keys,
            effective_pick_mode=?pick_mode,
            hint_present=cache_affinity_hint.is_some(),
            use_cache_affinity,
            hint_candidate_count=scoped_candidates.len(),
            hint_candidate_preview=?candidate_preview,
            eligible_credential_count=remaining.len(),
            "cache affinity selection start"
        );
    }

    let mut attempts = 0usize;
    let mut last_credential_id = None;
    let mut last_status = None;
    let mut last_error = None;
    let mut last_request_meta = None;

    while !remaining.is_empty() {
        let remaining_before_pick = remaining.len();
        let (idx, matched_affinity_idx) =
            pick_candidate_index(&remaining, &scoped_candidates, now_unix_ms, pick_mode);
        let candidate = remaining.swap_remove(idx);
        attempts += 1;
        if affinity_trace_enabled() {
            let matched_key = matched_affinity_idx
                .and_then(|i| scoped_candidates.get(i))
                .map(|item| item.scoped_key.as_str())
                .unwrap_or("-");
            tracing::info!(
                channel=%provider.channel.as_str(),
                model=%normalized_model.as_deref().unwrap_or("-"),
                attempt=attempts,
                remaining_before_pick,
                picked_credential_id=candidate.credential_id,
                matched_affinity=matched_affinity_idx.is_some(),
                matched_affinity_key=%matched_key,
                "cache affinity picked credential"
            );
        }
        match attempt(CredentialAttempt {
            credential_id: candidate.credential_id,
            material: candidate.material,
            attempts,
        })
        .await
        {
            CredentialRetryDecision::Return(value) => {
                if use_cache_affinity {
                    if let Some(bind) = scoped_bind.as_ref() {
                        bind_affinity(
                            provider.channel.as_str(),
                            bind.scoped_key.as_str(),
                            candidate.credential_id,
                            now_unix_ms.saturating_add(bind.ttl_ms),
                            now_unix_ms,
                            provider.cache_affinity_max_keys,
                        );
                    }
                    if let Some(matched_idx) = matched_affinity_idx
                        && let Some(hit) = scoped_candidates.get(matched_idx)
                    {
                        bind_affinity(
                            provider.channel.as_str(),
                            hit.scoped_key.as_str(),
                            candidate.credential_id,
                            now_unix_ms.saturating_add(hit.ttl_ms),
                            now_unix_ms,
                            provider.cache_affinity_max_keys,
                        );
                    }
                }
                if affinity_trace_enabled() {
                    let bind_key = scoped_bind
                        .as_ref()
                        .map(|item| item.scoped_key.as_str())
                        .unwrap_or("-");
                    let matched_key = matched_affinity_idx
                        .and_then(|i| scoped_candidates.get(i))
                        .map(|item| item.scoped_key.as_str())
                        .unwrap_or("-");
                    tracing::info!(
                        channel=%provider.channel.as_str(),
                        model=%normalized_model.as_deref().unwrap_or("-"),
                        attempt=attempts,
                        credential_id=candidate.credential_id,
                        bind_key=%bind_key,
                        matched_key=%matched_key,
                        "cache affinity credential succeeded"
                    );
                }
                return Ok(value);
            }
            CredentialRetryDecision::Retry {
                last_status: status,
                last_error: error,
                last_request_meta: request_meta,
            } => {
                if use_cache_affinity
                    && let Some(matched_idx) = matched_affinity_idx
                    && let Some(hit) = scoped_candidates.get(matched_idx)
                {
                    clear_affinity(hit.scoped_key.as_str());
                    if affinity_trace_enabled() {
                        tracing::info!(
                            channel=%provider.channel.as_str(),
                            model=%normalized_model.as_deref().unwrap_or("-"),
                            attempt=attempts,
                            credential_id=candidate.credential_id,
                            matched_key=%hit.scoped_key,
                            status=?status,
                            error=?error,
                            "cache affinity cleared matched key on retry"
                        );
                    }
                } else if affinity_trace_enabled() {
                    tracing::info!(
                        channel=%provider.channel.as_str(),
                        model=%normalized_model.as_deref().unwrap_or("-"),
                        attempt=attempts,
                        credential_id=candidate.credential_id,
                        status=?status,
                        error=?error,
                        "cache affinity retry without matched key clear"
                    );
                }
                last_credential_id = Some(candidate.credential_id);
                last_status = status;
                last_error = error;
                last_request_meta = request_meta;
            }
        }
    }

    if affinity_trace_enabled() {
        tracing::info!(
            channel=%provider.channel.as_str(),
            model=%normalized_model.as_deref().unwrap_or("-"),
            attempts,
            last_credential_id=?last_credential_id,
            last_status=?last_status,
            last_error=?last_error,
            "cache affinity exhausted all credentials"
        );
    }

    Err(UpstreamError::AllCredentialsExhausted {
        channel: provider.channel.as_str().to_string(),
        attempts,
        last_credential_id,
        last_status,
        last_error,
        last_request_meta: last_request_meta.map(Box::new),
    })
}

pub async fn retry_with_eligible_credentials<Material, T, Select, Attempt, Fut>(
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    model: Option<&str>,
    now_unix_ms: u64,
    select_material: Select,
    attempt: Attempt,
) -> Result<T, UpstreamError>
where
    Select: FnMut(&CredentialRef) -> Option<Material>,
    Attempt: FnMut(CredentialAttempt<Material>) -> Fut,
    Fut: Future<Output = CredentialRetryDecision<T>>,
{
    retry_with_eligible_credentials_with_affinity(
        CredentialRetryContext {
            provider,
            credential_states,
            model,
            now_unix_ms,
            pick_mode: CredentialPickMode::RoundRobinNoCache,
            cache_affinity_hint: None,
        },
        select_material,
        attempt,
    )
    .await
}

pub(super) fn scoped_affinity_key(provider: &ProviderDefinition, key: &str) -> String {
    format!("{}::{key}", provider.channel.as_str())
}

pub(super) fn scoped_affinity_candidates(
    provider: &ProviderDefinition,
    hint: &CacheAffinityHint,
) -> Vec<ScopedAffinityCandidate> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::with_capacity(hint.candidates.len());
    for candidate in &hint.candidates {
        let scoped_key = scoped_affinity_key(provider, candidate.key.as_str());
        if seen.insert(scoped_key.clone()) {
            candidates.push(ScopedAffinityCandidate {
                scoped_key,
                ttl_ms: candidate.ttl_ms,
                key_len: candidate.key_len,
            });
        }
    }
    candidates
}

pub(super) fn pick_candidate_index<Material>(
    remaining: &[CredentialCandidate<Material>],
    scoped_candidates: &[ScopedAffinityCandidate],
    now_unix_ms: u64,
    pick_mode: CredentialPickMode,
) -> (usize, Option<usize>) {
    if matches!(pick_mode, CredentialPickMode::RoundRobinWithCache) {
        let remaining_idx_by_credential = remaining
            .iter()
            .enumerate()
            .map(|(idx, item)| (item.credential_id, idx))
            .collect::<HashMap<_, _>>();
        let mut score_by_credential = HashMap::<i64, usize>::new();
        let mut representative_match = HashMap::<i64, (usize, usize)>::new();

        for (candidate_idx, candidate) in scoped_candidates.iter().enumerate() {
            let Some(credential_id) =
                get_affinity_credential_id(candidate.scoped_key.as_str(), now_unix_ms)
            else {
                continue;
            };

            if !remaining_idx_by_credential.contains_key(&credential_id) {
                continue;
            }

            let score = score_by_credential.entry(credential_id).or_default();
            *score = score.saturating_add(candidate.key_len);

            representative_match
                .entry(credential_id)
                .and_modify(|(best_idx, best_len)| {
                    if candidate.key_len > *best_len {
                        *best_idx = candidate_idx;
                        *best_len = candidate.key_len;
                    }
                })
                .or_insert((candidate_idx, candidate.key_len));
        }

        let mut best: Option<(usize, usize, usize)> = None;
        for (credential_id, score) in score_by_credential {
            let Some(&remaining_idx) = remaining_idx_by_credential.get(&credential_id) else {
                continue;
            };
            let matched_idx = representative_match
                .get(&credential_id)
                .map(|(idx, _)| *idx)
                .unwrap_or_default();

            match best {
                None => best = Some((remaining_idx, score, matched_idx)),
                Some((best_remaining_idx, best_score, _)) => {
                    if score > best_score
                        || (score == best_score && remaining_idx < best_remaining_idx)
                    {
                        best = Some((remaining_idx, score, matched_idx));
                    }
                }
            }
        }

        if let Some((remaining_idx, _, matched_idx)) = best {
            return (remaining_idx, Some(matched_idx));
        }

        return (rand::rng().random_range(0..remaining.len()), None);
    }

    if matches!(pick_mode, CredentialPickMode::RoundRobinNoCache) {
        return (rand::rng().random_range(0..remaining.len()), None);
    }

    let idx = remaining
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| candidate.credential_id)
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    (idx, None)
}

pub(super) fn get_affinity_credential_id(scoped_key: &str, now_unix_ms: u64) -> Option<i64> {
    let record = CACHE_AFFINITY.get(scoped_key)?;
    if record.expires_at_unix_ms <= now_unix_ms {
        if affinity_trace_enabled() {
            tracing::info!(
                scoped_key=%scoped_key,
                credential_id=record.credential_id,
                expires_at_unix_ms=record.expires_at_unix_ms,
                now_unix_ms,
                "cache affinity key expired"
            );
        }
        drop(record);
        CACHE_AFFINITY.remove(scoped_key);
        return None;
    }
    if affinity_trace_enabled() {
        tracing::info!(
            scoped_key=%scoped_key,
            credential_id=record.credential_id,
            expires_at_unix_ms=record.expires_at_unix_ms,
            now_unix_ms,
            "cache affinity key hit"
        );
    }
    Some(record.credential_id)
}

fn evict_affinity_keys_for_channel(
    channel: &str,
    incoming_scoped_key: &str,
    now_unix_ms: u64,
    max_keys: usize,
) {
    let mut expired_keys = Vec::new();
    let mut live_keys = Vec::new();
    for entry in CACHE_AFFINITY.iter() {
        let record = entry.value();
        if record.channel != channel {
            continue;
        }
        if record.expires_at_unix_ms <= now_unix_ms {
            expired_keys.push(entry.key().clone());
        } else {
            live_keys.push((entry.key().clone(), record.expires_at_unix_ms));
        }
    }

    for scoped_key in expired_keys {
        if affinity_trace_enabled() {
            tracing::info!(
                channel=%channel,
                scoped_key=%scoped_key,
                now_unix_ms,
                "cache affinity evict expired key while enforcing limit"
            );
        }
        CACHE_AFFINITY.remove(scoped_key.as_str());
    }

    let overflow = live_keys.len().saturating_add(1).saturating_sub(max_keys);
    if overflow == 0 {
        return;
    }

    live_keys.sort_unstable_by(
        |(left_key, left_expires_at), (right_key, right_expires_at)| {
            left_expires_at
                .cmp(right_expires_at)
                .then_with(|| left_key.cmp(right_key))
        },
    );

    for (scoped_key, expires_at_unix_ms) in live_keys
        .into_iter()
        .filter(|(scoped_key, _)| scoped_key != incoming_scoped_key)
        .take(overflow)
    {
        if affinity_trace_enabled() {
            tracing::info!(
                channel=%channel,
                scoped_key=%scoped_key,
                expires_at_unix_ms,
                max_keys,
                "cache affinity evict key due to per-channel limit"
            );
        }
        CACHE_AFFINITY.remove(scoped_key.as_str());
    }
}

pub(super) fn bind_affinity(
    channel: &str,
    scoped_key: &str,
    credential_id: i64,
    expires_at_unix_ms: u64,
    now_unix_ms: u64,
    max_keys: usize,
) {
    let existed = CACHE_AFFINITY.get(scoped_key).is_some();
    if !existed {
        evict_affinity_keys_for_channel(channel, scoped_key, now_unix_ms, max_keys);
    }
    if affinity_trace_enabled() {
        tracing::info!(
            channel=%channel,
            scoped_key=%scoped_key,
            credential_id,
            expires_at_unix_ms,
            max_keys,
            "cache affinity bind"
        );
    }
    CACHE_AFFINITY.insert(
        scoped_key.to_string(),
        CacheAffinityRecord {
            channel: channel.to_string(),
            credential_id,
            expires_at_unix_ms,
        },
    );
}

pub(super) fn clear_affinity(scoped_key: &str) {
    if affinity_trace_enabled() {
        tracing::info!(scoped_key=%scoped_key, "cache affinity clear");
    }
    CACHE_AFFINITY.remove(scoped_key);
}
