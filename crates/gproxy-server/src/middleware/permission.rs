use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};

/// A single permission entry for a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionEntry {
    /// None = applies to all providers.
    pub provider_id: Option<i64>,
    /// `*` = all models, `claude-*` = prefix match, exact string = exact match.
    pub model_pattern: String,
}

/// Shared permission table: user_id → list of permission entries.
pub type PermissionMap = Arc<ArcSwap<HashMap<i64, Vec<PermissionEntry>>>>;

/// Create a new empty permission map.
pub fn new_permission_map() -> PermissionMap {
    Arc::new(ArcSwap::from_pointee(HashMap::new()))
}

/// Check if a user is allowed to use a specific model on a provider.
///
/// Returns `false` if user has no permission entries at all (whitelist mode).
pub fn check_permission(
    map: &PermissionMap,
    user_id: i64,
    provider_id: i64,
    model: &str,
) -> bool {
    let perms = map.load();
    let Some(entries) = perms.get(&user_id) else {
        return false;
    };
    entries.iter().any(|e| {
        let provider_ok = e.provider_id.is_none() || e.provider_id == Some(provider_id);
        provider_ok && pattern_matches(&e.model_pattern, model)
    })
}

/// Match a model name against a pattern.
/// - `*` matches everything
/// - `claude-*` matches anything starting with `claude-`
/// - exact string matches exactly
pub fn pattern_matches(pattern: &str, model: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return model.starts_with(prefix);
    }
    pattern == model
}
