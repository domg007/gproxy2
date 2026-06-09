//! Authorization records (§8-C): scope-based route permissions, rate limits,
//! and quotas. Each is scoped to an org/team/user in the identity hierarchy.

use serde::{Deserialize, Serialize};

/// A route-access grant scoped to an org/team/user (§8-C).
///
/// `scope` is one of `org` | `team` | `user`; `scope_id` is the corresponding
/// entity id; `route_pattern` is the route/alias glob the scope may use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutePermission {
    pub id: i64,
    pub scope: String,
    pub scope_id: i64,
    pub route_pattern: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a route permission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutePermissionInput {
    pub id: Option<i64>,
    pub scope: String,
    pub scope_id: i64,
    pub route_pattern: String,
}

/// A rate limit scoped to an org/team/user for a route pattern (§8-C).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateLimit {
    pub id: i64,
    pub scope: String,
    pub scope_id: i64,
    pub route_pattern: String,
    pub rpm: Option<i64>,
    pub rpd: Option<i64>,
    pub total_tokens: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a rate limit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateLimitInput {
    pub id: Option<i64>,
    pub scope: String,
    pub scope_id: i64,
    pub route_pattern: String,
    pub rpm: Option<i64>,
    pub rpd: Option<i64>,
    pub total_tokens: Option<i64>,
}

/// A spend quota scoped to an org/team/user (§8-C). Unique per `(scope, scope_id)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quota {
    pub id: i64,
    pub scope: String,
    pub scope_id: i64,
    pub quota_total: f64,
    pub cost_used: f64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a quota.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaInput {
    pub id: Option<i64>,
    pub scope: String,
    pub scope_id: i64,
    pub quota_total: f64,
    pub cost_used: f64,
}
