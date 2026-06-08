//! Provider record (§8-B). A `provider` is one upstream endpoint family
//! (channel) with its settings and credential strategy.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A persisted upstream provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub id: i64,
    /// Unique provider name.
    pub name: String,
    /// Channel / adapter family, e.g. `openai`, `claude`, `gemini`, `openrouter`.
    pub channel: String,
    pub label: Option<String>,
    /// Free-form settings: `base_url`, channel scalar toggles, default
    /// `proxy_url`, `circuit_breaker`, etc. (see §7.4 / §3.2).
    pub settings_json: Value,
    /// Credential pool strategy: `round_robin` | `sticky` (§3.3).
    pub credential_strategy: String,
    pub enabled: bool,
    /// Unix seconds.
    pub created_at: i64,
    /// Unix seconds.
    pub updated_at: i64,
}

/// Upsert input for a provider. `id = None` inserts; `Some(id)` updates.
/// `created_at` / `updated_at` are managed by the backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderInput {
    pub id: Option<i64>,
    pub name: String,
    pub channel: String,
    pub label: Option<String>,
    pub settings_json: Value,
    pub credential_strategy: String,
    pub enabled: bool,
}
