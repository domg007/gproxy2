use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

use gproxy_sdk::provider::engine::{GproxyEngineBuilder, ProviderConfig};
use gproxy_server::{
    AppState, GlobalConfig, MemoryModel, MemoryUser, MemoryUserKey, ModelAliasTarget,
    PermissionEntry, RateLimitRule,
};

use crate::auth::authorize_admin;
use crate::error::HttpError;

// ---------------------------------------------------------------------------
// TOML schema
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct GproxyToml {
    #[serde(default)]
    pub global: Option<GlobalSettingsToml>,
    #[serde(default)]
    pub providers: Vec<ProviderToml>,
    #[serde(default)]
    pub models: Vec<ModelToml>,
    #[serde(default)]
    pub model_aliases: Vec<ModelAliasToml>,
    #[serde(default)]
    pub users: Vec<UserToml>,
    #[serde(default)]
    pub permissions: Vec<PermissionToml>,
    #[serde(default)]
    pub rate_limits: Vec<RateLimitToml>,
    #[serde(default)]
    pub quotas: Vec<QuotaToml>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GlobalSettingsToml {
    pub host: String,
    pub port: u16,
    pub admin_key: String,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default = "default_spoof")]
    pub spoof_emulation: String,
    #[serde(default = "default_update_source")]
    pub update_source: String,
    #[serde(default = "default_true")]
    pub enable_usage: bool,
    #[serde(default = "default_true")]
    pub enable_upstream_log: bool,
    #[serde(default)]
    pub enable_upstream_log_body: bool,
    #[serde(default = "default_true")]
    pub enable_downstream_log: bool,
    #[serde(default)]
    pub enable_downstream_log_body: bool,
    pub dsn: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
}

fn default_spoof() -> String {
    "chrome_136".to_string()
}
fn default_update_source() -> String {
    "github".to_string()
}
fn default_true() -> bool {
    true
}
fn default_data_dir() -> String {
    "./data".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProviderToml {
    pub name: String,
    pub channel: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub settings: serde_json::Value,
    #[serde(default)]
    pub credentials: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelToml {
    pub provider_name: String,
    pub model_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub price_each_call: Option<f64>,
    #[serde(default)]
    pub price_input_tokens: Option<f64>,
    #[serde(default)]
    pub price_output_tokens: Option<f64>,
    #[serde(default)]
    pub price_cache_read_input_tokens: Option<f64>,
    #[serde(default)]
    pub price_cache_creation_input_tokens: Option<f64>,
    #[serde(default)]
    pub price_cache_creation_input_tokens_5min: Option<f64>,
    #[serde(default)]
    pub price_cache_creation_input_tokens_1h: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelAliasToml {
    pub alias: String,
    pub provider_name: String,
    pub model_id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserToml {
    pub name: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub keys: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PermissionToml {
    pub user_name: String,
    #[serde(default)]
    pub provider_name: Option<String>,
    pub model_pattern: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RateLimitToml {
    pub user_name: String,
    pub model_pattern: String,
    #[serde(default)]
    pub rpm: Option<i32>,
    #[serde(default)]
    pub rpd: Option<i32>,
    #[serde(default)]
    pub total_tokens: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QuotaToml {
    pub user_name: String,
    pub quota: f64,
    #[serde(default)]
    pub cost_used: f64,
}

// ---------------------------------------------------------------------------
// Export: memory → TOML
// ---------------------------------------------------------------------------

pub async fn export_toml(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<String, HttpError> {
    authorize_admin(&headers, &state)?;

    let config = state.config();
    let engine = state.engine();
    let store = engine.store();

    // Global settings
    let global = GlobalSettingsToml {
        host: config.host.clone(),
        port: config.port,
        admin_key: config.admin_key.clone(),
        proxy: config.proxy.clone(),
        spoof_emulation: config.spoof_emulation.clone(),
        update_source: config.update_source.clone(),
        enable_usage: config.enable_usage,
        enable_upstream_log: config.enable_upstream_log,
        enable_upstream_log_body: config.enable_upstream_log_body,
        enable_downstream_log: config.enable_downstream_log,
        enable_downstream_log_body: config.enable_downstream_log_body,
        dsn: config.dsn.clone(),
        data_dir: config.data_dir.clone(),
    };

    // Providers + credentials from SDK store
    let provider_snapshots = store
        .list_providers()
        .map_err(|e| HttpError::internal(e.to_string()))?;
    let mut providers = Vec::new();
    for p in &provider_snapshots {
        let creds = store
            .list_credentials(Some(&p.name))
            .map_err(|e| HttpError::internal(e.to_string()))?;
        providers.push(ProviderToml {
            name: p.name.clone(),
            channel: p.channel.clone(),
            enabled: true,
            settings: p.settings.clone(),
            credentials: creds.into_iter().map(|c| c.credential).collect(),
        });
    }

    // Models
    let memory_models = state.models();
    let models: Vec<ModelToml> = memory_models
        .iter()
        .map(|m| ModelToml {
            provider_name: m.provider_id.to_string(),
            model_id: m.model_id.clone(),
            display_name: m.display_name.clone(),
            enabled: m.enabled,
            price_each_call: m.price_each_call,
            price_input_tokens: m.price_input_tokens,
            price_output_tokens: m.price_output_tokens,
            price_cache_read_input_tokens: m.price_cache_read_input_tokens,
            price_cache_creation_input_tokens: m.price_cache_creation_input_tokens,
            price_cache_creation_input_tokens_5min: m.price_cache_creation_input_tokens_5min,
            price_cache_creation_input_tokens_1h: m.price_cache_creation_input_tokens_1h,
        })
        .collect();

    // Model aliases
    let alias_snapshot = state.model_aliases_snapshot();
    let model_aliases: Vec<ModelAliasToml> = alias_snapshot
        .iter()
        .map(|(alias, target)| ModelAliasToml {
            alias: alias.clone(),
            provider_name: target.provider_name.clone(),
            model_id: target.model_id.clone(),
            enabled: true,
        })
        .collect();

    // Users + keys
    let users_snapshot = state.users_snapshot();
    let keys_snapshot = state.keys_snapshot();
    let users: Vec<UserToml> = users_snapshot
        .iter()
        .map(|u| {
            let user_keys: Vec<String> = keys_snapshot
                .values()
                .filter(|k| k.user_id == u.id && k.enabled)
                .map(|k| k.api_key.clone())
                .collect();
            UserToml {
                name: u.name.clone(),
                password: String::new(), // Don't export passwords
                enabled: u.enabled,
                keys: user_keys,
            }
        })
        .collect();

    // Permissions
    let perms_snapshot = state.user_permissions_snapshot();
    let user_name_map: std::collections::HashMap<i64, String> = users_snapshot
        .iter()
        .map(|u| (u.id, u.name.clone()))
        .collect();
    let mut permissions = Vec::new();
    for (user_id, entries) in perms_snapshot.iter() {
        let user_name = user_name_map.get(user_id).cloned().unwrap_or_default();
        for e in entries {
            permissions.push(PermissionToml {
                user_name: user_name.clone(),
                provider_name: e.provider_id.map(|id| id.to_string()),
                model_pattern: e.model_pattern.clone(),
            });
        }
    }

    // Rate limits
    let limits_snapshot = state.user_rate_limits_snapshot();
    let mut rate_limits = Vec::new();
    for (user_id, rules) in limits_snapshot.iter() {
        let user_name = user_name_map.get(user_id).cloned().unwrap_or_default();
        for r in rules {
            rate_limits.push(RateLimitToml {
                user_name: user_name.clone(),
                model_pattern: r.model_pattern.clone(),
                rpm: r.rpm,
                rpd: r.rpd,
                total_tokens: r.total_tokens,
            });
        }
    }

    // Quotas
    let quota_map = state.user_quotas_snapshot();
    let quotas: Vec<QuotaToml> = quota_map
        .iter()
        .map(|(user_id, (quota, cost_used))| QuotaToml {
            user_name: user_name_map.get(user_id).cloned().unwrap_or_default(),
            quota: *quota,
            cost_used: *cost_used,
        })
        .collect();

    let toml = GproxyToml {
        global: Some(global),
        providers,
        models,
        model_aliases,
        users,
        permissions,
        rate_limits,
        quotas,
    };

    toml::to_string_pretty(&toml).map_err(|e| HttpError::internal(e.to_string()))
}

// ---------------------------------------------------------------------------
// Import: TOML → memory + database
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ImportPayload {
    pub toml: String,
}

pub async fn import_toml(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ImportPayload>,
) -> Result<Json<crate::error::AckResponse>, HttpError> {
    authorize_admin(&headers, &state)?;

    let config: GproxyToml = toml::from_str(&payload.toml)
        .map_err(|e| HttpError::bad_request(format!("invalid TOML: {e}")))?;

    // 1. Global settings
    if let Some(gs) = &config.global {
        state.replace_config(GlobalConfig {
            host: gs.host.clone(),
            port: gs.port,
            admin_key: gs.admin_key.clone(),
            proxy: gs.proxy.clone(),
            spoof_emulation: gs.spoof_emulation.clone(),
            update_source: gs.update_source.clone(),
            enable_usage: gs.enable_usage,
            enable_upstream_log: gs.enable_upstream_log,
            enable_upstream_log_body: gs.enable_upstream_log_body,
            enable_downstream_log: gs.enable_downstream_log,
            enable_downstream_log_body: gs.enable_downstream_log_body,
            dsn: gs.dsn.clone(),
            data_dir: gs.data_dir.clone(),
        });
    }

    // 2. Rebuild engine from providers
    let proxy = config.global.as_ref().and_then(|g| g.proxy.as_deref());
    let spoof = config.global.as_ref().map(|g| g.spoof_emulation.as_str());
    let mut builder = GproxyEngineBuilder::new().configure_clients(proxy, spoof);
    for p in &config.providers {
        let pc = ProviderConfig {
            name: p.name.clone(),
            channel: p.channel.clone(),
            settings_json: p.settings.clone(),
            credentials: p.credentials.clone(),
        };
        builder = builder
            .add_provider_json(pc)
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    state.replace_engine(builder.build());

    // 3. Models
    let models: Vec<MemoryModel> = config
        .models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            MemoryModel {
                id: i as i64 + 1,
                provider_id: 0, // Resolved by name at bootstrap
                model_id: m.model_id.clone(),
                display_name: m.display_name.clone(),
                enabled: m.enabled,
                price_each_call: m.price_each_call,
                price_input_tokens: m.price_input_tokens,
                price_output_tokens: m.price_output_tokens,
                price_cache_read_input_tokens: m.price_cache_read_input_tokens,
                price_cache_creation_input_tokens: m.price_cache_creation_input_tokens,
                price_cache_creation_input_tokens_5min: m.price_cache_creation_input_tokens_5min,
                price_cache_creation_input_tokens_1h: m.price_cache_creation_input_tokens_1h,
            }
        })
        .collect();
    state.replace_models(models);

    // 4. Model aliases
    let aliases = config
        .model_aliases
        .iter()
        .filter(|a| a.enabled)
        .map(|a| {
            (
                a.alias.clone(),
                ModelAliasTarget {
                    provider_name: a.provider_name.clone(),
                    model_id: a.model_id.clone(),
                },
            )
        })
        .collect();
    state.replace_model_aliases(aliases);

    // 5. Users + keys
    for (i, u) in config.users.iter().enumerate() {
        let user_id = i as i64 + 1;
        state.upsert_user_in_memory(MemoryUser {
            id: user_id,
            name: u.name.clone(),
            enabled: u.enabled,
        });
        for (j, key) in u.keys.iter().enumerate() {
            state.upsert_key_in_memory(MemoryUserKey {
                id: (user_id * 1000) + j as i64,
                user_id,
                api_key: key.clone(),
                enabled: true,
            });
        }
    }

    // Build user name → id map
    let users_snapshot = state.users_snapshot();
    let user_id_map: std::collections::HashMap<String, i64> = users_snapshot
        .iter()
        .map(|u| (u.name.clone(), u.id))
        .collect();

    // 6. Permissions
    let mut perm_map: std::collections::HashMap<i64, Vec<PermissionEntry>> =
        std::collections::HashMap::new();
    for p in &config.permissions {
        if let Some(&user_id) = user_id_map.get(&p.user_name) {
            perm_map.entry(user_id).or_default().push(PermissionEntry {
                provider_id: None, // TODO: resolve provider_name to id
                model_pattern: p.model_pattern.clone(),
            });
        }
    }
    state.replace_user_permissions(perm_map);

    // 7. Rate limits
    let mut limit_map: std::collections::HashMap<i64, Vec<RateLimitRule>> =
        std::collections::HashMap::new();
    for r in &config.rate_limits {
        if let Some(&user_id) = user_id_map.get(&r.user_name) {
            limit_map.entry(user_id).or_default().push(RateLimitRule {
                model_pattern: r.model_pattern.clone(),
                rpm: r.rpm,
                rpd: r.rpd,
                total_tokens: r.total_tokens,
            });
        }
    }
    state.replace_user_rate_limits(limit_map);

    // 8. Quotas
    let quota_map: std::collections::HashMap<i64, (f64, f64)> = config
        .quotas
        .iter()
        .filter_map(|q| {
            let user_id = user_id_map.get(&q.user_name)?;
            Some((*user_id, (q.quota, q.cost_used)))
        })
        .collect();
    state.replace_user_quotas(quota_map);

    Ok(Json(crate::error::AckResponse { ok: true, id: None }))
}
