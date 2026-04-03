use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;

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

#[derive(Debug)]
pub enum RateLimitRejection {
    Rpm { limit: i32 },
    Rpd { limit: i32 },
    QuotaExhausted { quota: f64, cost_used: f64 },
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

    pub fn check_and_increment(&self, user_id: i64, model: &str) {
        let key = (user_id, model.to_string());
        Self::increment(&self.minute, &key, MINUTE);
        Self::increment(&self.day, &key, DAY);
    }

    pub fn check_rpm(&self, user_id: i64, model: &str) -> u32 {
        Self::check(&self.minute, &(user_id, model.to_string()), MINUTE)
    }

    pub fn check_rpd(&self, user_id: i64, model: &str) -> u32 {
        Self::check(&self.day, &(user_id, model.to_string()), DAY)
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
// Axum middleware
// ---------------------------------------------------------------------------

/// Axum middleware placeholder for rate limit enforcement.
///
/// Rate limiting is currently done inside the provider handler
/// (after authentication and model resolution).
/// This middleware is a pass-through reserved for future use.
pub async fn rate_limit_middleware(
    State(_state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    next.run(request).await
}

// ---------------------------------------------------------------------------
// Logic (called by AppState convenience methods)
// ---------------------------------------------------------------------------

/// Find the most specific matching rule. Priority: exact > prefix wildcard > `*`.
pub fn find_matching_rule<'a>(
    rules: &'a [RateLimitRule],
    model: &str,
) -> Option<&'a RateLimitRule> {
    if let Some(r) = rules.iter().find(|r| r.model_pattern == model) {
        return Some(r);
    }
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
    rules.iter().find(|r| r.model_pattern == "*")
}
