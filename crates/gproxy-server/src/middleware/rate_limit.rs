use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration types
// ---------------------------------------------------------------------------

/// Rate limit rule for a user+model pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitRule {
    pub model_pattern: String,
    pub rpm: Option<i32>,
    pub rpd: Option<i32>,
    pub total_tokens: Option<i64>,
}

/// Shared rate limit config: user_id → rules.
pub type RateLimitConfigMap = Arc<ArcSwap<HashMap<i64, Vec<RateLimitRule>>>>;

/// Shared user quota tracking: user_id → (tokens_used, cost_used).
pub type UserQuotaMap = Arc<ArcSwap<HashMap<i64, (i64, f64)>>>;

/// Create a new empty rate limit config map.
pub fn new_rate_limit_config_map() -> RateLimitConfigMap {
    Arc::new(ArcSwap::from_pointee(HashMap::new()))
}

/// Create a new empty user quota map.
pub fn new_user_quota_map() -> UserQuotaMap {
    Arc::new(ArcSwap::from_pointee(HashMap::new()))
}

// ---------------------------------------------------------------------------
// Rejection
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum RateLimitRejection {
    Rpm { limit: i32 },
    Rpd { limit: i32 },
    TokenQuota { used: i64, limit: i64 },
}

// ---------------------------------------------------------------------------
// Counters (in-memory, not persisted)
// ---------------------------------------------------------------------------

const MINUTE: Duration = Duration::from_secs(60);
const DAY: Duration = Duration::from_secs(86400);

/// Sliding-window rate limit counters. Not persisted — resets on restart.
pub struct RateLimitCounters {
    minute: DashMap<(i64, String), WindowCounter>,
    day: DashMap<(i64, String), WindowCounter>,
}

struct WindowCounter {
    count: u32,
    window_start: Instant,
}

impl RateLimitCounters {
    pub fn new() -> Self {
        Self {
            minute: DashMap::new(),
            day: DashMap::new(),
        }
    }

    fn check(
        map: &DashMap<(i64, String), WindowCounter>,
        key: &(i64, String),
        window: Duration,
    ) -> u32 {
        let Some(entry) = map.get(key) else {
            return 0;
        };
        if entry.window_start.elapsed() >= window {
            0
        } else {
            entry.count
        }
    }

    fn increment(
        map: &DashMap<(i64, String), WindowCounter>,
        key: &(i64, String),
        window: Duration,
    ) {
        let mut entry = map.entry(key.clone()).or_insert(WindowCounter {
            count: 0,
            window_start: Instant::now(),
        });
        if entry.window_start.elapsed() >= window {
            entry.count = 1;
            entry.window_start = Instant::now();
        } else {
            entry.count += 1;
        }
    }
}

impl Default for RateLimitCounters {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check rate limit for a user+model request.
/// Returns `Ok(())` if allowed, `Err(rejection)` if blocked.
pub fn check_rate_limit(
    config: &RateLimitConfigMap,
    quotas: &UserQuotaMap,
    counters: &RateLimitCounters,
    user_id: i64,
    model: &str,
) -> Result<(), RateLimitRejection> {
    let limits = config.load();
    let Some(user_limits) = limits.get(&user_id) else {
        return Ok(());
    };
    let Some(rule) = find_matching_rule(user_limits, model) else {
        return Ok(());
    };

    let key = (user_id, model.to_string());

    if let Some(rpm) = rule.rpm {
        let count = RateLimitCounters::check(&counters.minute, &key, MINUTE);
        if count >= rpm as u32 {
            return Err(RateLimitRejection::Rpm { limit: rpm });
        }
    }
    if let Some(rpd) = rule.rpd {
        let count = RateLimitCounters::check(&counters.day, &key, DAY);
        if count >= rpd as u32 {
            return Err(RateLimitRejection::Rpd { limit: rpd });
        }
    }
    if let Some(total_tokens) = rule.total_tokens {
        let used = quotas
            .load()
            .get(&user_id)
            .map(|(t, _)| *t)
            .unwrap_or(0);
        if used >= total_tokens {
            return Err(RateLimitRejection::TokenQuota {
                used,
                limit: total_tokens,
            });
        }
    }
    Ok(())
}

/// Record a request for RPM/RPD counting.
pub fn record_request(counters: &RateLimitCounters, user_id: i64, model: &str) {
    let key = (user_id, model.to_string());
    RateLimitCounters::increment(&counters.minute, &key, MINUTE);
    RateLimitCounters::increment(&counters.day, &key, DAY);
}

/// Update token usage for a user (in-memory).
pub fn add_token_usage(quotas: &UserQuotaMap, user_id: i64, tokens: i64, cost: f64) {
    let mut map = (*quotas.load_full()).clone();
    let entry = map.entry(user_id).or_insert((0, 0.0));
    entry.0 = entry.0.saturating_add(tokens);
    entry.1 += cost;
    quotas.store(Arc::new(map));
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

/// Find the most specific matching rule. Priority: exact > prefix wildcard > `*`.
fn find_matching_rule<'a>(rules: &'a [RateLimitRule], model: &str) -> Option<&'a RateLimitRule> {
    // Exact
    if let Some(r) = rules.iter().find(|r| r.model_pattern == model) {
        return Some(r);
    }
    // Longest prefix wildcard
    let mut best: Option<&RateLimitRule> = None;
    let mut best_len = 0;
    for rule in rules {
        if let Some(prefix) = rule.model_pattern.strip_suffix('*')
            && model.starts_with(prefix)
            && prefix.len() > best_len
        {
            best = Some(rule);
            best_len = prefix.len();
        }
    }
    if best.is_some() {
        return best;
    }
    // Fallback: `*`
    rules.iter().find(|r| r.model_pattern == "*")
}
