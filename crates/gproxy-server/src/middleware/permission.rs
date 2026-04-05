use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;

/// A single permission entry for a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionEntry {
    /// Stable database identity for admin CRUD and cache synchronization.
    pub id: i64,
    /// None = applies to all providers.
    pub provider_id: Option<i64>,
    /// `*` = all models, `claude-*` = prefix match, exact string = exact match.
    pub model_pattern: String,
}

/// A provider-scoped file API permission for a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePermissionEntry {
    /// Stable database identity for admin CRUD and cache synchronization.
    pub id: i64,
    pub provider_id: i64,
}

/// Axum middleware placeholder for permission checks.
///
/// Permission enforcement is currently done inside the provider handler
/// (after authentication extracts user_id and model resolution is complete).
/// This middleware is a pass-through reserved for future use.
pub async fn permission_middleware(
    State(_state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    next.run(request).await
}

/// Match a model name against a pattern.
pub fn pattern_matches(pattern: &str, model: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return model.starts_with(prefix);
    }
    pattern == model
}
