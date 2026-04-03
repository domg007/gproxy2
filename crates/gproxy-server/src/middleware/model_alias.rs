use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};

/// Target of a model alias resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAliasTarget {
    pub provider_name: String,
    pub model_id: String,
}

/// Shared model alias table. Pass `Arc<ArcSwap<...>>` to the middleware,
/// update it from AppState when admin changes aliases.
pub type ModelAliasMap = Arc<ArcSwap<HashMap<String, ModelAliasTarget>>>;

/// Create a new empty alias map.
pub fn new_model_alias_map() -> ModelAliasMap {
    Arc::new(ArcSwap::from_pointee(HashMap::new()))
}

/// Resolve a model name through the alias table.
/// Returns `Some(target)` if an enabled alias exists, `None` otherwise.
pub fn resolve_alias(map: &ModelAliasMap, model: &str) -> Option<ModelAliasTarget> {
    map.load().get(model).cloned()
}
