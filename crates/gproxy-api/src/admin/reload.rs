use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde::Serialize;

use gproxy_sdk::provider::engine::{GproxyEngineBuilder, ProviderConfig};
use gproxy_server::{
    AppState, MemoryModel, MemoryUser, MemoryUserKey, ModelAliasTarget, PermissionEntry,
    RateLimitRule,
};

use crate::auth::authorize_admin;
use crate::error::HttpError;

#[derive(Serialize)]
pub struct ReloadResponse {
    pub ok: bool,
    pub users: usize,
    pub keys: usize,
    pub models: usize,
    pub aliases: usize,
    pub permissions: usize,
    pub rate_limits: usize,
    pub quotas: usize,
}

/// Reload all in-memory caches from the database.
pub async fn reload(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ReloadResponse>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.storage();

    // Load global settings first (need proxy/spoof for HTTP clients)
    let config = if let Some(settings) = storage.get_global_settings().await? {
        let cfg = gproxy_server::GlobalConfig {
            host: settings.host,
            port: settings.port as u16,
            admin_key: settings.admin_key,
            proxy: settings.proxy,
            spoof_emulation: settings.spoof_emulation.unwrap_or_default(),
            update_source: settings.update_source.unwrap_or_default(),
            enable_usage: settings.enable_usage,
            enable_upstream_log: settings.enable_upstream_log,
            enable_upstream_log_body: settings.enable_upstream_log_body,
            enable_downstream_log: settings.enable_downstream_log,
            enable_downstream_log_body: settings.enable_downstream_log_body,
            dsn: settings.dsn,
            data_dir: settings.data_dir,
        };
        state.replace_config(cfg.clone());
        cfg
    } else {
        (*state.config()).clone()
    };

    // Rebuild engine from database providers + credentials
    let providers = storage
        .list_providers(&gproxy_storage::ProviderQuery::default())
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;
    let all_credentials = storage
        .list_credentials(&gproxy_storage::CredentialQuery::default())
        .await
        .map_err(|e| HttpError::internal(e.to_string()))?;

    let mut builder = GproxyEngineBuilder::new().configure_clients(
        config.proxy.as_deref(),
        Some(config.spoof_emulation.as_str()),
    );
    for provider in &providers {
        if !provider.enabled {
            continue;
        }
        let creds: Vec<serde_json::Value> = all_credentials
            .iter()
            .filter(|c| c.provider_id == provider.id && c.enabled)
            .map(|c| c.secret_json.clone())
            .collect();
        let config = ProviderConfig {
            name: provider.name.clone(),
            channel: provider.channel.clone(),
            settings_json: provider.settings_json.clone(),
            credentials: creds,
        };
        builder = builder
            .add_provider_json(config)
            .map_err(|e| HttpError::internal(e.to_string()))?;
    }
    let new_engine = builder.build();
    state.replace_engine(new_engine);

    // Users
    let users = storage
        .list_users(&gproxy_storage::UserQuery::default())
        .await?;
    let user_count = users.len();
    let memory_users: Vec<MemoryUser> = users
        .iter()
        .map(|u| MemoryUser {
            id: u.id,
            name: u.name.clone(),
            enabled: u.enabled,
        })
        .collect();
    for u in &memory_users {
        state.upsert_user_in_memory(u.clone());
    }

    // User keys
    let keys = storage.list_user_keys_for_memory().await?;
    let key_count = keys.len();
    for k in &keys {
        state.upsert_key_in_memory(MemoryUserKey {
            id: k.id,
            user_id: k.user_id,
            api_key: k.api_key.clone(),
            enabled: k.enabled,
        });
    }

    // Models
    let models = storage
        .list_models(&gproxy_storage::ModelQuery::default())
        .await?;
    let model_count = models.len();
    let memory_models: Vec<MemoryModel> = models
        .iter()
        .map(|m| MemoryModel {
            id: m.id,
            provider_id: m.provider_id,
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
    state.replace_models(memory_models);

    // Model aliases
    let aliases = storage
        .list_model_aliases(&gproxy_storage::ModelAliasQuery::default())
        .await?;
    let alias_count = aliases.len();
    // Build provider_id -> name map from the providers already loaded above
    let provider_name_map: std::collections::HashMap<i64, String> =
        providers.iter().map(|p| (p.id, p.name.clone())).collect();
    let alias_map = aliases
        .into_iter()
        .filter(|a| a.enabled)
        .map(|a| {
            let provider_name = provider_name_map
                .get(&a.provider_id)
                .cloned()
                .unwrap_or_else(|| a.provider_id.to_string());
            (
                a.alias,
                ModelAliasTarget {
                    provider_name,
                    model_id: a.model_id,
                },
            )
        })
        .collect();
    state.replace_model_aliases(alias_map);

    // Permissions
    let perms = storage
        .list_user_model_permissions(&gproxy_storage::UserModelPermissionQuery::default())
        .await?;
    let perm_count = perms.len();
    let mut perm_map = std::collections::HashMap::new();
    for p in perms {
        perm_map
            .entry(p.user_id)
            .or_insert_with(Vec::new)
            .push(PermissionEntry {
                provider_id: p.provider_id,
                model_pattern: p.model_pattern,
            });
    }
    state.replace_user_permissions(perm_map);

    // Rate limits
    let limits = storage
        .list_user_rate_limits(&gproxy_storage::UserRateLimitQuery::default())
        .await?;
    let limit_count = limits.len();
    let mut limit_map = std::collections::HashMap::new();
    for l in limits {
        limit_map
            .entry(l.user_id)
            .or_insert_with(Vec::new)
            .push(RateLimitRule {
                model_pattern: l.model_pattern,
                rpm: l.rpm,
                rpd: l.rpd,
                total_tokens: l.total_tokens,
            });
    }
    state.replace_user_rate_limits(limit_map);

    // Quotas
    let quotas = storage.list_user_quotas().await?;
    let quota_count = quotas.len();
    let quota_map = quotas
        .into_iter()
        .map(|q| (q.user_id, (q.quota, q.cost_used)))
        .collect();
    state.replace_user_quotas(quota_map);

    Ok(Json(ReloadResponse {
        ok: true,
        users: user_count,
        keys: key_count,
        models: model_count,
        aliases: alias_count,
        permissions: perm_count,
        rate_limits: limit_count,
        quotas: quota_count,
    }))
}
