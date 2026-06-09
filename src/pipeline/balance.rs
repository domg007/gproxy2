//! Candidate selection (§3.3, §6.4). M1: take the lowest-tier members in their
//! pre-sorted order and pair each with its provider's enabled credentials.
//! Real strategies + health/breaker filtering land in M3.

use std::sync::Arc;

use crate::app::snapshot::{ControlPlaneSnapshot, ResolvedRoute};
use crate::pipeline::context::Candidate;
use crate::pipeline::error::PipelineError;
use crate::store::cache::CacheBackend;

/// Build the ordered candidate list for failover. Members are already sorted by
/// `(tier asc, weight desc)`; M1 restricts to the lowest tier and expands each
/// member across its credential pool.
pub fn candidates(
    cp: &ControlPlaneSnapshot,
    route: &ResolvedRoute,
    _cache: &dyn CacheBackend,
    _affinity_key: Option<&str>,
) -> Result<Vec<Candidate>, PipelineError> {
    let lowest_tier = route.members.first().ok_or(PipelineError::NoMembers)?.tier;

    let mut out = Vec::new();
    for member in route.members.iter().filter(|m| m.tier == lowest_tier) {
        let Some(provider) = cp.providers_by_id.get(&member.provider_id) else {
            continue;
        };
        if !provider.enabled {
            continue;
        }
        let Some(creds) = cp.credentials_by_provider.get(&member.provider_id) else {
            continue;
        };
        for cred in creds {
            out.push(Candidate {
                provider: Arc::clone(provider),
                credential: Arc::clone(cred),
                upstream_model_id: member.upstream_model_id.clone(),
            });
        }
    }

    if out.is_empty() {
        return Err(PipelineError::NoCredentials);
    }
    Ok(out)
}
