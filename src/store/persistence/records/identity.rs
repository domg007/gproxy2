//! Identity & authorization records (§8-C): orgs, teams, users, user keys,
//! and scope-based route permissions / rate limits / quotas.

use serde::{Deserialize, Serialize};

/// An organization — the top of the identity hierarchy (§8-C).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Org {
    pub id: i64,
    /// Unique org name.
    pub name: String,
    pub enabled: bool,
    pub description: Option<String>,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds.
    pub updated_at: i64,
}

/// Upsert input for an org. `id = None` inserts; `Some(id)` updates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrgInput {
    pub id: Option<i64>,
    pub name: String,
    pub enabled: bool,
    pub description: Option<String>,
}

/// A team within an org (§8-C). Unique per `(org_id, name)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Team {
    pub id: i64,
    pub org_id: i64,
    pub name: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a team.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamInput {
    pub id: Option<i64>,
    pub org_id: i64,
    pub name: String,
    pub enabled: bool,
}

/// A user belonging to an org and (optionally) a team (§8-C).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    /// Unique user name.
    pub name: String,
    pub org_id: i64,
    pub team_id: Option<i64>,
    pub password: Option<String>,
    pub enabled: bool,
    pub is_admin: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserInput {
    pub id: Option<i64>,
    pub name: String,
    pub org_id: i64,
    pub team_id: Option<i64>,
    pub password: Option<String>,
    pub enabled: bool,
    pub is_admin: bool,
}

/// An API key issued to a user (§8-C). `api_key_digest` is unique.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserKey {
    pub id: i64,
    pub user_id: i64,
    pub api_key_ciphertext: String,
    pub api_key_digest: String,
    pub label: Option<String>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a user key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserKeyInput {
    pub id: Option<i64>,
    pub user_id: i64,
    pub api_key_ciphertext: String,
    pub api_key_digest: String,
    pub label: Option<String>,
    pub enabled: bool,
}

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
