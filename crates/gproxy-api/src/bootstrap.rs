//! Bootstrap logic shared by the startup path and admin reload/import endpoints.

use std::collections::HashMap;

use gproxy_sdk::provider::engine::{GproxyEngineBuilder, ProviderConfig};
use gproxy_server::{
    AppState, GlobalConfig, MemoryModel, MemoryUser, MemoryUserKey, ModelAliasTarget,
    PermissionEntry, PriceTier, RateLimitRule,
};
use gproxy_storage::StorageWriteEvent;

use crate::admin::config_toml::GproxyToml;

/// Counts of items loaded during a reload.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ReloadCounts {
    pub providers: usize,
    pub users: usize,
    pub keys: usize,
    pub models: usize,
    pub aliases: usize,
    pub permissions: usize,
    pub rate_limits: usize,
    pub quotas: usize,
}

/// Reload all in-memory state from the database.
///
/// Used by both the initial bootstrap and the `POST /admin/reload` endpoint.
pub async fn reload_from_db(state: &AppState) -> Result<ReloadCounts, Box<dyn std::error::Error + Send + Sync>> {
    let storage = state.storage();

    // Global settings
    if let Some(settings) = storage.get_global_settings().await? {
        state.replace_config(GlobalConfig {
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
        });
    }
    let config = state.config().clone();

    // Providers + credentials → engine
    let providers = storage
        .list_providers(&gproxy_storage::ProviderQuery::default())
        .await?;
    let all_credentials = storage
        .list_credentials(&gproxy_storage::CredentialQuery::default())
        .await?;

    let mut builder = GproxyEngineBuilder::new().configure_clients(
        config.proxy.as_deref(),
        Some(config.spoof_emulation.as_str()),
    );
    let mut provider_count = 0;
    for provider in &providers {
        if !provider.enabled {
            continue;
        }
        let creds: Vec<serde_json::Value> = all_credentials
            .iter()
            .filter(|c| c.provider_id == provider.id && c.enabled)
            .map(|c| c.secret_json.clone())
            .collect();
        builder = builder.add_provider_json(ProviderConfig {
            name: provider.name.clone(),
            channel: provider.channel.clone(),
            settings_json: provider.settings_json.clone(),
            credentials: creds,
        })?;
        provider_count += 1;
    }
    state.replace_engine(builder.build());

    // Users
    let users = storage
        .list_users(&gproxy_storage::UserQuery::default())
        .await?;
    let user_count = users.len();
    for u in &users {
        state.upsert_user_in_memory(MemoryUser {
            id: u.id,
            name: u.name.clone(),
            enabled: u.enabled,
        });
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
        .map(|m| {
            let price_tiers: Vec<PriceTier> = m
                .price_tiers_json
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            MemoryModel {
                id: m.id,
                provider_id: m.provider_id,
                model_id: m.model_id.clone(),
                display_name: m.display_name.clone(),
                enabled: m.enabled,
                price_each_call: m.price_each_call,
                price_tiers,
            }
        })
        .collect();
    state.replace_models(memory_models);

    // Model aliases
    let aliases = storage
        .list_model_aliases(&gproxy_storage::ModelAliasQuery::default())
        .await?;
    let alias_count = aliases.len();
    let provider_name_map: HashMap<i64, String> =
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
    let mut perm_map: HashMap<i64, Vec<PermissionEntry>> = HashMap::new();
    for p in perms {
        perm_map
            .entry(p.user_id)
            .or_default()
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
    let mut limit_map: HashMap<i64, Vec<RateLimitRule>> = HashMap::new();
    for l in limits {
        limit_map
            .entry(l.user_id)
            .or_default()
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
    let quota_map: HashMap<i64, (f64, f64)> = quotas
        .into_iter()
        .map(|q| (q.user_id, (q.quota, q.cost_used)))
        .collect();
    state.replace_user_quotas(quota_map);

    Ok(ReloadCounts {
        providers: provider_count,
        users: user_count,
        keys: key_count,
        models: model_count,
        aliases: alias_count,
        permissions: perm_count,
        rate_limits: limit_count,
        quotas: quota_count,
    })
}

/// Import a TOML config string into memory AND persist everything to the database.
///
/// Used by the initial bootstrap when seeding from a TOML file.
pub async fn seed_from_toml(
    state: &AppState,
    toml_str: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config: GproxyToml = toml::from_str(toml_str)?;

    // 1. Global settings → memory + DB
    if let Some(gs) = &config.global {
        let gc = GlobalConfig {
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
        };
        state.replace_config(gc);
    }
    // Persist global settings
    let cfg = state.config().clone();
    state
        .storage_writes()
        .enqueue(StorageWriteEvent::UpsertGlobalSettings(
            gproxy_storage::GlobalSettingsWrite {
                host: cfg.host.clone(),
                port: cfg.port,
                admin_key: cfg.admin_key.clone(),
                proxy: cfg.proxy.clone(),
                spoof_emulation: cfg.spoof_emulation.clone(),
                update_source: cfg.update_source.clone(),
                enable_usage: cfg.enable_usage,
                enable_upstream_log: cfg.enable_upstream_log,
                enable_upstream_log_body: cfg.enable_upstream_log_body,
                enable_downstream_log: cfg.enable_downstream_log,
                enable_downstream_log_body: cfg.enable_downstream_log_body,
                dsn: cfg.dsn.clone(),
                data_dir: cfg.data_dir.clone(),
            },
        ))
        .await?;

    // 2. Providers → engine + DB
    let proxy = config.global.as_ref().and_then(|g| g.proxy.as_deref());
    let spoof = config.global.as_ref().map(|g| g.spoof_emulation.as_str());
    let mut builder = GproxyEngineBuilder::new().configure_clients(proxy, spoof);
    for (i, p) in config.providers.iter().enumerate() {
        builder = builder.add_provider_json(ProviderConfig {
            name: p.name.clone(),
            channel: p.channel.clone(),
            settings_json: p.settings.clone(),
            credentials: p.credentials.clone(),
        })?;
        // Persist provider
        state
            .storage_writes()
            .enqueue(StorageWriteEvent::UpsertProvider(
                gproxy_storage::ProviderWrite {
                    id: i as i64 + 1,
                    name: p.name.clone(),
                    channel: p.channel.clone(),
                    enabled: p.enabled,
                    settings_json: serde_json::to_string(&p.settings).unwrap_or_default(),
                    dispatch_json: String::new(),
                },
            ))
            .await?;
        // Persist credentials
        for (j, cred) in p.credentials.iter().enumerate() {
            state
                .storage_writes()
                .enqueue(StorageWriteEvent::UpsertCredential(
                    gproxy_storage::CredentialWrite {
                        id: (i as i64 + 1) * 1000 + j as i64,
                        provider_id: i as i64 + 1,
                        name: None,
                        kind: p.channel.clone(),
                        enabled: true,
                        secret_json: serde_json::to_string(cred).unwrap_or_default(),
                    },
                ))
                .await?;
        }
    }
    state.replace_engine(builder.build());

    // Provider name → synthetic id
    let provider_name_to_id: HashMap<String, i64> = config
        .providers
        .iter()
        .enumerate()
        .map(|(i, p)| (p.name.clone(), i as i64 + 1))
        .collect();

    // 3. Users → memory + DB
    for (i, u) in config.users.iter().enumerate() {
        let user_id = i as i64 + 1;
        let hashed_password = crate::login::hash_password(&u.password);
        state.upsert_user_in_memory(MemoryUser {
            id: user_id,
            name: u.name.clone(),
            enabled: u.enabled,
        });
        state
            .storage_writes()
            .enqueue(StorageWriteEvent::UpsertUser(gproxy_storage::UserWrite {
                id: user_id,
                name: u.name.clone(),
                password: hashed_password,
                enabled: u.enabled,
            }))
            .await?;
        for (j, key) in u.keys.iter().enumerate() {
            let key_id = user_id * 1000 + j as i64;
            state.upsert_key_in_memory(MemoryUserKey {
                id: key_id,
                user_id,
                api_key: key.clone(),
                enabled: true,
            });
            state
                .storage_writes()
                .enqueue(StorageWriteEvent::UpsertUserKey(
                    gproxy_storage::UserKeyWrite {
                        id: key_id,
                        user_id,
                        api_key: key.clone(),
                        label: None,
                        enabled: true,
                    },
                ))
                .await?;
        }
    }

    // 4. Models → memory + DB
    let models: Vec<MemoryModel> = config
        .models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let provider_id = provider_name_to_id
                .get(&m.provider_name)
                .copied()
                .unwrap_or(0);
            MemoryModel {
                id: i as i64 + 1,
                provider_id,
                model_id: m.model_id.clone(),
                display_name: m.display_name.clone(),
                enabled: m.enabled,
                price_each_call: m.price_each_call,
                price_tiers: m.price_tiers.clone(),
            }
        })
        .collect();
    for m in &models {
        state
            .storage_writes()
            .enqueue(StorageWriteEvent::UpsertModel(gproxy_storage::ModelWrite {
                id: m.id,
                provider_id: m.provider_id,
                model_id: m.model_id.clone(),
                display_name: m.display_name.clone(),
                enabled: m.enabled,
                price_each_call: m.price_each_call,
                price_tiers_json: if m.price_tiers.is_empty() {
                    None
                } else {
                    serde_json::to_string(&m.price_tiers).ok()
                },
            }))
            .await?;
    }
    state.replace_models(models);

    // 5. Model aliases → memory + DB
    let mut alias_map = HashMap::new();
    for (i, a) in config.model_aliases.iter().enumerate() {
        if !a.enabled {
            continue;
        }
        let provider_id = provider_name_to_id
            .get(&a.provider_name)
            .copied()
            .unwrap_or(0);
        state
            .storage_writes()
            .enqueue(StorageWriteEvent::UpsertModelAlias(
                gproxy_storage::ModelAliasWrite {
                    id: i as i64 + 1,
                    alias: a.alias.clone(),
                    provider_id,
                    model_id: a.model_id.clone(),
                    enabled: true,
                },
            ))
            .await?;
        alias_map.insert(
            a.alias.clone(),
            ModelAliasTarget {
                provider_name: a.provider_name.clone(),
                model_id: a.model_id.clone(),
            },
        );
    }
    state.replace_model_aliases(alias_map);

    // 6. Permissions, rate limits, quotas → memory + DB
    let users_snapshot = state.users_snapshot();
    let user_id_map: HashMap<String, i64> =
        users_snapshot.iter().map(|u| (u.name.clone(), u.id)).collect();

    let mut perm_map: HashMap<i64, Vec<PermissionEntry>> = HashMap::new();
    for (i, p) in config.permissions.iter().enumerate() {
        if let Some(&user_id) = user_id_map.get(&p.user_name) {
            let provider_id = p
                .provider_name
                .as_ref()
                .and_then(|name| provider_name_to_id.get(name).copied());
            perm_map.entry(user_id).or_default().push(PermissionEntry {
                provider_id,
                model_pattern: p.model_pattern.clone(),
            });
            state
                .storage_writes()
                .enqueue(StorageWriteEvent::UpsertUserModelPermission(
                    gproxy_storage::UserModelPermissionWrite {
                        id: i as i64 + 1,
                        user_id,
                        provider_id,
                        model_pattern: p.model_pattern.clone(),
                    },
                ))
                .await?;
        }
    }
    state.replace_user_permissions(perm_map);

    let mut limit_map: HashMap<i64, Vec<RateLimitRule>> = HashMap::new();
    for (i, r) in config.rate_limits.iter().enumerate() {
        if let Some(&user_id) = user_id_map.get(&r.user_name) {
            limit_map.entry(user_id).or_default().push(RateLimitRule {
                model_pattern: r.model_pattern.clone(),
                rpm: r.rpm,
                rpd: r.rpd,
                total_tokens: r.total_tokens,
            });
            state
                .storage_writes()
                .enqueue(StorageWriteEvent::UpsertUserRateLimit(
                    gproxy_storage::UserRateLimitWrite {
                        id: i as i64 + 1,
                        user_id,
                        model_pattern: r.model_pattern.clone(),
                        rpm: r.rpm,
                        rpd: r.rpd,
                        total_tokens: r.total_tokens,
                    },
                ))
                .await?;
        }
    }
    state.replace_user_rate_limits(limit_map);

    let mut quota_map: HashMap<i64, (f64, f64)> = HashMap::new();
    for q in &config.quotas {
        if let Some(&user_id) = user_id_map.get(&q.user_name) {
            state
                .storage_writes()
                .enqueue(StorageWriteEvent::UpsertUserQuota(
                    gproxy_storage::UserQuotaWrite {
                        user_id,
                        quota: q.quota,
                        cost_used: q.cost_used,
                    },
                ))
                .await?;
            quota_map.insert(user_id, (q.quota, q.cost_used));
        }
    }
    state.replace_user_quotas(quota_map);

    Ok(())
}

/// Seed the database with minimal defaults (global_settings only).
pub async fn seed_defaults(
    state: &AppState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cfg = state.config().clone();
    state
        .storage_writes()
        .enqueue(StorageWriteEvent::UpsertGlobalSettings(
            gproxy_storage::GlobalSettingsWrite {
                host: cfg.host.clone(),
                port: cfg.port,
                admin_key: cfg.admin_key.clone(),
                proxy: cfg.proxy.clone(),
                spoof_emulation: cfg.spoof_emulation.clone(),
                update_source: cfg.update_source.clone(),
                enable_usage: cfg.enable_usage,
                enable_upstream_log: cfg.enable_upstream_log,
                enable_upstream_log_body: cfg.enable_upstream_log_body,
                enable_downstream_log: cfg.enable_downstream_log,
                enable_downstream_log_body: cfg.enable_downstream_log_body,
                dsn: cfg.dsn.clone(),
                data_dir: cfg.data_dir.clone(),
            },
        ))
        .await?;
    Ok(())
}
