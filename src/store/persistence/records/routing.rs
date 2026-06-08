//! Routing records (§8-A): routes, members, aliases, provider models.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A route — one logical model name backed by 1..N members (§3.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub id: i64,
    pub name: String,
    /// `weighted` | `round_robin` | `failover` | `least_latency`.
    pub strategy: String,
    pub enabled: bool,
    pub description: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteInput {
    pub id: Option<i64>,
    pub name: String,
    pub strategy: String,
    pub enabled: bool,
    pub description: Option<String>,
}

/// A member of a route's backend pool (§3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteMember {
    pub id: i64,
    pub route_id: i64,
    pub provider_id: i64,
    pub upstream_model_id: String,
    pub weight: i64,
    pub tier: i64,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a route member.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteMemberInput {
    pub id: Option<i64>,
    pub route_id: i64,
    pub provider_id: i64,
    pub upstream_model_id: String,
    pub weight: i64,
    pub tier: i64,
    pub enabled: bool,
}

/// An alias (name → route, many-to-one; §3.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alias {
    pub id: i64,
    pub alias: String,
    pub route_id: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for an alias.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AliasInput {
    pub id: Option<i64>,
    pub alias: String,
    pub route_id: i64,
}

/// A model exposed by a provider, with optional pricing (§8-A).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderModel {
    pub id: i64,
    pub provider_id: i64,
    pub model_id: String,
    pub display_name: Option<String>,
    pub pricing_json: Option<Value>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a provider model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderModelInput {
    pub id: Option<i64>,
    pub provider_id: i64,
    pub model_id: String,
    pub display_name: Option<String>,
    pub pricing_json: Option<Value>,
    pub enabled: bool,
}
