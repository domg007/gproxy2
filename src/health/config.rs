//! Circuit-breaker thresholds (§3.2), parsed from
//! `providers.settings_json.circuit_breaker`. Route-level override is
//! deferred (routes have no settings column yet).

use serde::Deserialize;

/// Hard cap on the exponentially-backed-off cooldown.
pub const COOLDOWN_CAP_SECS: u64 = 300;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BreakerConfig {
    /// Open after this many consecutive failures.
    pub consecutive_failures: u32,
    /// Optional windowed error-rate trigger (off by default).
    pub error_rate: Option<ErrorRateCfg>,
    /// Base cooldown; doubles per consecutive open, capped at
    /// [`COOLDOWN_CAP_SECS`].
    pub cooldown_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorRateCfg {
    pub window_secs: u64,
    pub threshold: f64,
    pub min_requests: u32,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            consecutive_failures: 5,
            error_rate: None,
            cooldown_secs: 30,
        }
    }
}

/// Parse from provider settings; absent → default; malformed → default + warn.
pub fn breaker_config(provider_settings: &serde_json::Value) -> BreakerConfig {
    match provider_settings.get("circuit_breaker") {
        None | Some(serde_json::Value::Null) => BreakerConfig::default(),
        Some(v) => serde_json::from_value(v.clone()).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "malformed circuit_breaker settings; using defaults");
            BreakerConfig::default()
        }),
    }
}
