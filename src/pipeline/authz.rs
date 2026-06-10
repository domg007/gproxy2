//! Three-level authz (§8-C): permission union, rate-limit precheck, quota
//! precheck — org → team → user, inserted after route resolution and before
//! balance. Counters live in the cache (redis-direct in multi-instance);
//! nothing here reads persistence on the hot path.

use std::time::Duration;

use crate::app::snapshot::{ControlPlaneSnapshot, KeyIdentity};
use crate::pipeline::error::PipelineError;
use crate::store::cache::CacheBackend;
use crate::store::persistence::records::Scope;
use crate::util::glob;

const MINUTE: i64 = 60;
const DAY: i64 = 86_400;

/// The identity's scope chain, most-specific first (check order §8-C).
fn scopes(identity: &KeyIdentity) -> Vec<(Scope, i64)> {
    let user = &identity.user;
    let mut chain = Vec::with_capacity(3);
    chain.push((Scope::User, user.id));
    if let Some(team_id) = user.team_id {
        chain.push((Scope::Team, team_id));
    }
    chain.push((Scope::Org, user.org_id));
    chain
}

/// 403 unless the org (and team, when set) is enabled AND the permission
/// union matches `name`. No matching pattern anywhere = deny (secure default).
pub fn check_permission(
    cp: &ControlPlaneSnapshot,
    identity: &KeyIdentity,
    name: &str,
) -> Result<(), PipelineError> {
    let user = &identity.user;
    match cp.orgs_by_id.get(&user.org_id) {
        Some(org) if org.enabled => {}
        _ => return Err(PipelineError::Forbidden),
    }
    if let Some(team_id) = user.team_id {
        match cp.teams_by_id.get(&team_id) {
            Some(team) if team.enabled => {}
            _ => return Err(PipelineError::Forbidden),
        }
    }
    // Effective permission = UNION of user ∪ team ∪ org patterns.
    for scope in scopes(identity) {
        if let Some(patterns) = cp.permissions_by_scope.get(&scope)
            && patterns.iter().any(|p| glob::matches(p, name))
        {
            return Ok(());
        }
    }
    Err(PipelineError::Forbidden)
}

/// user → team → org; first exceeded rule wins. Incr-then-check: the
/// rejected request is still counted (cheap, deterministic — no read-modify-
/// write race, at the cost of rejected requests consuming budget).
pub async fn precheck_limits(
    cp: &ControlPlaneSnapshot,
    cache: &dyn CacheBackend,
    identity: &KeyIdentity,
    name: &str,
    now_unix: i64,
) -> Result<(), PipelineError> {
    for scope in scopes(identity) {
        let Some(rows) = cp.rate_limits_by_scope.get(&scope) else {
            continue;
        };
        for row in rows
            .iter()
            .filter(|r| glob::matches(&r.route_pattern, name))
        {
            if let Some(limit) = row.rpm {
                let key = format!("rl:{}:m{}", row.id, now_unix / MINUTE);
                let count = cache.incr(&key, 1, Some(Duration::from_secs(120))).await;
                if count > limit {
                    return Err(PipelineError::RateLimited {
                        retry_after_secs: (MINUTE - now_unix % MINUTE) as u64,
                    });
                }
            }
            if let Some(limit) = row.rpd {
                let key = format!("rl:{}:d{}", row.id, now_unix / DAY);
                let count = cache
                    .incr(&key, 1, Some(Duration::from_secs(48 * 3600)))
                    .await;
                if count > limit {
                    return Err(PipelineError::RateLimited {
                        retry_after_secs: (DAY - now_unix % DAY) as u64,
                    });
                }
            }
            if let Some(limit) = row.total_tokens {
                // Read-only precheck of the daily token budget: incr(_, 0)
                // just reads the counter. Nothing increments `rlt:*` until
                // M6 wires post-response token accounting in here.
                let key = format!("rlt:{}:d{}", row.id, now_unix / DAY);
                let count = cache
                    .incr(&key, 0, Some(Duration::from_secs(48 * 3600)))
                    .await;
                if count > limit {
                    return Err(PipelineError::RateLimited {
                        retry_after_secs: (DAY - now_unix % DAY) as u64,
                    });
                }
            }
        }
    }
    Ok(())
}

/// All scope quotas must have cost_used < quota_total (M6 adds estimate
/// deduction + reconciliation).
pub fn precheck_quota(
    cp: &ControlPlaneSnapshot,
    identity: &KeyIdentity,
) -> Result<(), PipelineError> {
    for scope in scopes(identity) {
        if let Some(quota) = cp.quotas_by_scope.get(&scope)
            && quota.cost_used >= quota.quota_total
        {
            return Err(PipelineError::QuotaExceeded);
        }
    }
    Ok(())
}

/// The single pipeline entry point: permission → limits → quota.
pub async fn authorize(
    cp: &ControlPlaneSnapshot,
    cache: &dyn CacheBackend,
    identity: &KeyIdentity,
    name: &str,
    now_unix: i64,
) -> Result<(), PipelineError> {
    check_permission(cp, identity, name)?;
    precheck_limits(cp, cache, identity, name, now_unix).await?;
    precheck_quota(cp, identity)
}

#[cfg(all(test, not(target_arch = "wasm32"), feature = "cache-memory"))]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::cache::MemoryCache;
    use crate::store::persistence::records::{Org, RateLimit, User, UserKey};

    fn test_identity() -> KeyIdentity {
        KeyIdentity {
            user_key: UserKey {
                id: 1,
                user_id: 1,
                api_key_ciphertext: String::new(),
                api_key_digest: "d".into(),
                label: None,
                enabled: true,
                created_at: 0,
                updated_at: 0,
            },
            user: User {
                id: 1,
                name: "u".into(),
                org_id: 10,
                team_id: None,
                password: None,
                enabled: true,
                is_admin: false,
                created_at: 0,
                updated_at: 0,
            },
        }
    }

    fn org(enabled: bool) -> Org {
        Org {
            id: 10,
            name: "o".into(),
            enabled,
            description: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[tokio::test]
    async fn org_level_grant_unions_down() {
        let identity = test_identity();
        let mut cp = ControlPlaneSnapshot::empty(1);

        // No org row at all → deny (secure default).
        assert!(matches!(
            check_permission(&cp, &identity, "claude-main"),
            Err(PipelineError::Forbidden)
        ));

        cp.orgs_by_id.insert(10, Arc::new(org(true)));
        // Org enabled but no permission rows anywhere → still deny.
        assert!(matches!(
            check_permission(&cp, &identity, "claude-main"),
            Err(PipelineError::Forbidden)
        ));

        cp.permissions_by_scope
            .insert((Scope::Org, 10), Arc::new(vec!["claude-*".into()]));
        assert!(check_permission(&cp, &identity, "claude-main").is_ok());
        assert!(matches!(
            check_permission(&cp, &identity, "gpt-x"),
            Err(PipelineError::Forbidden)
        ));
    }

    #[tokio::test]
    async fn disabled_org_denies() {
        let identity = test_identity();
        let mut cp = ControlPlaneSnapshot::empty(1);
        cp.orgs_by_id.insert(10, Arc::new(org(false)));
        cp.permissions_by_scope
            .insert((Scope::User, 1), Arc::new(vec!["*".into()]));
        assert!(matches!(
            check_permission(&cp, &identity, "claude-main"),
            Err(PipelineError::Forbidden)
        ));
    }

    #[tokio::test]
    async fn rpm_trips_and_retry_after() {
        let identity = test_identity();
        let mut cp = ControlPlaneSnapshot::empty(1);
        cp.rate_limits_by_scope.insert(
            (Scope::User, 1),
            Arc::new(vec![RateLimit {
                id: 7,
                scope: Scope::User,
                scope_id: 1,
                route_pattern: "*".into(),
                rpm: Some(2),
                rpd: None,
                total_tokens: None,
                created_at: 0,
                updated_at: 0,
            }]),
        );
        let cache = MemoryCache::new();
        let now = 1_000_000;
        for _ in 0..2 {
            precheck_limits(&cp, &cache, &identity, "claude-main", now)
                .await
                .expect("under limit");
        }
        match precheck_limits(&cp, &cache, &identity, "claude-main", now).await {
            Err(PipelineError::RateLimited { retry_after_secs }) => {
                assert!((1..=60).contains(&retry_after_secs));
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }
}
