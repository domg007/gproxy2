//! Provider-scoped rule records (§8-B): routing, rewrite, sanitize, cache
//! breakpoints, beta headers, and system preludes. All are scoped to one
//! provider and carry `sort_order` + `enabled`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A per-provider routing rule mapping an `(operation, kind)` to an
/// implementation, optionally rewriting the destination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingRule {
    pub id: i64,
    pub provider_id: i64,
    pub operation: String,
    pub kind: String,
    pub implementation: String,
    pub dest_operation: Option<String>,
    pub dest_kind: Option<String>,
    pub sort_order: i64,
    pub enabled: bool,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds.
    pub updated_at: i64,
}

/// Upsert input for a routing rule (unique per `(provider_id, operation, kind)`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingRuleInput {
    pub id: Option<i64>,
    pub provider_id: i64,
    pub operation: String,
    pub kind: String,
    pub implementation: String,
    pub dest_operation: Option<String>,
    pub dest_kind: Option<String>,
    pub sort_order: i64,
    pub enabled: bool,
}

/// A per-provider request/response rewrite rule applied at a JSON path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RewriteRule {
    pub id: i64,
    pub provider_id: i64,
    pub path: String,
    pub action: String,
    #[serde(default)]
    pub value_json: Option<Value>,
    pub filter_model_pattern: Option<String>,
    #[serde(default)]
    pub filter_operation_keys: Option<Value>,
    pub sort_order: i64,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a rewrite rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RewriteRuleInput {
    pub id: Option<i64>,
    pub provider_id: i64,
    pub path: String,
    pub action: String,
    #[serde(default)]
    pub value_json: Option<Value>,
    pub filter_model_pattern: Option<String>,
    #[serde(default)]
    pub filter_operation_keys: Option<Value>,
    pub sort_order: i64,
    pub enabled: bool,
}

/// A per-provider sanitize rule (regex `pattern` → `replacement`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SanitizeRule {
    pub id: i64,
    pub provider_id: i64,
    pub pattern: String,
    pub replacement: String,
    pub sort_order: i64,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a sanitize rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SanitizeRuleInput {
    pub id: Option<i64>,
    pub provider_id: i64,
    pub pattern: String,
    pub replacement: String,
    pub sort_order: i64,
    pub enabled: bool,
}

/// A per-provider prompt-cache breakpoint marker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheBreakpoint {
    pub id: i64,
    pub provider_id: i64,
    pub target: String,
    pub position: String,
    pub index: i64,
    pub ttl: String,
    pub sort_order: i64,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a cache breakpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheBreakpointInput {
    pub id: Option<i64>,
    pub provider_id: i64,
    pub target: String,
    pub position: String,
    pub index: i64,
    pub ttl: String,
    pub sort_order: i64,
    pub enabled: bool,
}

/// A per-provider beta header token to inject.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaHeader {
    pub id: i64,
    pub provider_id: i64,
    pub token: String,
    pub sort_order: i64,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a beta header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BetaHeaderInput {
    pub id: Option<i64>,
    pub provider_id: i64,
    pub token: String,
    pub sort_order: i64,
    pub enabled: bool,
}

/// A per-provider system prelude snippet prepended to the system prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreludeSystem {
    pub id: i64,
    pub provider_id: i64,
    pub text: String,
    pub sort_order: i64,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Upsert input for a system prelude.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreludeSystemInput {
    pub id: Option<i64>,
    pub provider_id: i64,
    pub text: String,
    pub sort_order: i64,
    pub enabled: bool,
}
