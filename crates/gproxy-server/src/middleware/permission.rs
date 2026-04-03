use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;

/// A single permission entry for a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionEntry {
    /// None = applies to all providers.
    pub provider_id: Option<i64>,
    /// `*` = all models, `claude-*` = prefix match, exact string = exact match.
    pub model_pattern: String,
}

/// Axum middleware: check user model permissions (whitelist).
///
/// Requires `AuthContext` (user_id) and model info in extensions.
/// Returns 403 if user is not authorized for this model/provider.
pub async fn permission_middleware(
    State(_state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    // Get user_id and model from extensions
    // These would be set by auth and request_model middlewares
    // For now, pass through — actual enforcement happens when we have auth middleware
    // TODO: extract user_id from auth context extension, check permission
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
