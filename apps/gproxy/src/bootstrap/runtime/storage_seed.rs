use anyhow::{Context, Result};
use gproxy_core::GlobalSettings;
use gproxy_provider::{
    BUILTIN_CHANNELS, BuiltinChannel, ChannelCredential, ChannelCredentialState, ChannelId,
    CredentialPickMode, CredentialRef, DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS,
    ProviderCredentialState, ProviderDefinition, ProviderDispatchTable, ProviderRegistry,
    credential_health_from_storage, credential_health_to_storage, credential_kind_for_storage,
    parse_credential_cache_affinity_max_keys_from_provider_settings_value,
    parse_credential_pick_mode_from_provider_settings_value,
    provider_settings_to_json_string_with_routing,
};
use gproxy_storage::{
    CredentialQuery, CredentialStatusQuery, CredentialStatusWrite, CredentialWrite,
    GlobalSettingsWrite, ProviderQuery, ProviderWrite, Scope, SeaOrmStorage, StorageWriteBatch,
    StorageWriteEvent, StorageWriteSink,
};

use crate::bootstrap::config::BootstrapConfig;

use super::registry::resolve_provider_settings;

const BUILTIN_PROVIDER_ID_OPENAI: i64 = 1;
const BUILTIN_PROVIDER_ID_ANTHROPIC: i64 = 2;
const BUILTIN_PROVIDER_ID_AISTUDIO: i64 = 3;
const BUILTIN_PROVIDER_ID_VERTEXEXPRESS: i64 = 4;
const BUILTIN_PROVIDER_ID_VERTEX: i64 = 5;
const BUILTIN_PROVIDER_ID_GEMINICLI: i64 = 6;
const BUILTIN_PROVIDER_ID_CLAUDECODE: i64 = 7;
const BUILTIN_PROVIDER_ID_CODEX: i64 = 8;
const BUILTIN_PROVIDER_ID_ANTIGRAVITY: i64 = 9;
const BUILTIN_PROVIDER_ID_NVIDIA: i64 = 10;
const BUILTIN_PROVIDER_ID_DEEPSEEK: i64 = 11;
const BUILTIN_PROVIDER_ID_GROQ: i64 = 12;
const CUSTOM_PROVIDER_ID_START: i64 = 1000;

fn default_builtin_provider_id(channel: BuiltinChannel) -> i64 {
    match channel {
        BuiltinChannel::OpenAi => BUILTIN_PROVIDER_ID_OPENAI,
        BuiltinChannel::Anthropic => BUILTIN_PROVIDER_ID_ANTHROPIC,
        BuiltinChannel::AiStudio => BUILTIN_PROVIDER_ID_AISTUDIO,
        BuiltinChannel::VertexExpress => BUILTIN_PROVIDER_ID_VERTEXEXPRESS,
        BuiltinChannel::Vertex => BUILTIN_PROVIDER_ID_VERTEX,
        BuiltinChannel::GeminiCli => BUILTIN_PROVIDER_ID_GEMINICLI,
        BuiltinChannel::ClaudeCode => BUILTIN_PROVIDER_ID_CLAUDECODE,
        BuiltinChannel::Codex => BUILTIN_PROVIDER_ID_CODEX,
        BuiltinChannel::Antigravity => BUILTIN_PROVIDER_ID_ANTIGRAVITY,
        BuiltinChannel::Nvidia => BUILTIN_PROVIDER_ID_NVIDIA,
        BuiltinChannel::Deepseek => BUILTIN_PROVIDER_ID_DEEPSEEK,
        BuiltinChannel::Groq => BUILTIN_PROVIDER_ID_GROQ,
    }
}

fn claim_next_available_provider_id(
    used_ids: &mut std::collections::HashSet<i64>,
    next_id: &mut i64,
) -> i64 {
    while used_ids.contains(next_id) {
        *next_id += 1;
    }
    let id = *next_id;
    used_ids.insert(id);
    *next_id += 1;
    id
}

pub(super) async fn seed_registry_providers(
    storage: &SeaOrmStorage,
    config: &BootstrapConfig,
    registry: &mut ProviderRegistry,
) -> Result<(
    std::collections::HashMap<String, i64>,
    std::collections::BTreeMap<String, bool>,
)> {
    let existing = storage
        .list_providers(&ProviderQuery {
            channel: Scope::All,
            name: Scope::All,
            enabled: Scope::All,
            limit: None,
        })
        .await
        .context("list existing providers for bootstrap seed")?;

    let mut id_by_channel = existing
        .iter()
        .map(|row| (row.channel.clone(), row.id))
        .collect::<std::collections::HashMap<_, _>>();
    let mut used_ids = existing
        .iter()
        .map(|row| row.id)
        .collect::<std::collections::HashSet<_>>();
    let mut next_builtin_fallback_id = existing.iter().map(|row| row.id).max().unwrap_or(-1) + 1;
    let mut next_custom_id = next_builtin_fallback_id.max(CUSTOM_PROVIDER_ID_START);

    let mut enabled_by_channel = existing
        .iter()
        .map(|row| (row.channel.clone(), row.enabled))
        .collect::<std::collections::BTreeMap<_, _>>();

    // Prefer existing storage provider settings/pick mode/dispatch so runtime does
    // not overwrite admin updates on restart when config.toml is absent.
    let mut provider_by_channel = std::collections::BTreeMap::<String, ProviderDefinition>::new();
    for row in &existing {
        let channel = ChannelId::parse(row.channel.as_str());
        let dispatch = serde_json::from_value::<ProviderDispatchTable>(row.dispatch_json.clone())
            .unwrap_or_else(|err| {
                eprintln!(
                    "bootstrap: invalid dispatch for channel={} ({err}), fallback to default",
                    row.channel
                );
                match channel {
                    ChannelId::Builtin(builtin) => {
                        ProviderDispatchTable::default_for_builtin(builtin)
                    }
                    ChannelId::Custom(_) => ProviderDispatchTable::default_for_custom(),
                }
            });
        provider_by_channel.insert(
            row.channel.clone(),
            ProviderDefinition {
                channel: channel.clone(),
                dispatch,
                settings: resolve_provider_settings(&channel, &row.settings_json),
                credential_pick_mode: parse_credential_pick_mode_from_provider_settings_value(
                    &row.settings_json,
                ),
                cache_affinity_max_keys:
                    parse_credential_cache_affinity_max_keys_from_provider_settings_value(
                        &row.settings_json,
                    )
                    .unwrap_or(DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS),
                credentials: ProviderCredentialState::default(),
            },
        );
    }

    // Ensure builtin channels always exist.
    for builtin in BUILTIN_CHANNELS {
        let channel_id = ChannelId::builtin(builtin);
        enabled_by_channel
            .entry(channel_id.as_str().to_string())
            .or_insert(false);
        provider_by_channel
            .entry(channel_id.as_str().to_string())
            .or_insert_with(|| ProviderDefinition {
                channel: channel_id.clone(),
                dispatch: ProviderDispatchTable::default_for_builtin(builtin),
                settings: resolve_provider_settings(&channel_id, &serde_json::json!({})),
                credential_pick_mode: CredentialPickMode::RoundRobinWithCache,
                cache_affinity_max_keys: DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS,
                credentials: ProviderCredentialState::default(),
            });
    }

    // config.toml providers (if present) override storage values.
    for provider in &registry.providers {
        provider_by_channel.insert(provider.channel.as_str().to_string(), provider.clone());
    }
    for channel_cfg in &config.channels {
        if channel_cfg.id.trim().is_empty() {
            continue;
        }
        enabled_by_channel.insert(channel_cfg.id.trim().to_string(), channel_cfg.enabled);
    }

    let mut batch = StorageWriteBatch::default();
    for (channel, provider) in provider_by_channel.iter() {
        let id = if let Some(id) = id_by_channel.get(channel.as_str()).copied() {
            id
        } else {
            let id = match ChannelId::parse(channel.as_str()) {
                ChannelId::Builtin(builtin) => {
                    let preferred = default_builtin_provider_id(builtin);
                    if used_ids.contains(&preferred) {
                        claim_next_available_provider_id(
                            &mut used_ids,
                            &mut next_builtin_fallback_id,
                        )
                    } else {
                        used_ids.insert(preferred);
                        preferred
                    }
                }
                ChannelId::Custom(_) => {
                    claim_next_available_provider_id(&mut used_ids, &mut next_custom_id)
                }
            };
            id_by_channel.insert(channel.clone(), id);
            id
        };

        let settings_json = provider_settings_to_json_string_with_routing(
            &provider.settings,
            provider.credential_pick_mode,
            provider.cache_affinity_max_keys,
        )
        .context("serialize provider settings for bootstrap seed")?;
        let dispatch_json = serde_json::to_string(&provider.dispatch)
            .context("serialize provider dispatch for bootstrap seed")?;
        batch.apply(StorageWriteEvent::UpsertProvider(ProviderWrite {
            id,
            name: channel.to_string(),
            channel: channel.to_string(),
            settings_json,
            dispatch_json,
            enabled: enabled_by_channel.get(channel).copied().unwrap_or(false),
        }));
    }

    if !batch.is_empty() {
        storage
            .write_batch(batch)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    }

    registry.providers = provider_by_channel.into_values().collect();

    Ok((id_by_channel, enabled_by_channel))
}

pub(super) async fn seed_global_settings(
    storage: &SeaOrmStorage,
    global: &GlobalSettings,
) -> Result<()> {
    let mut batch = StorageWriteBatch::default();
    batch.apply(StorageWriteEvent::UpsertGlobalSettings(
        GlobalSettingsWrite {
            host: global.host.clone(),
            port: global.port,
            hf_token: global.hf_token.clone(),
            hf_url: global.hf_url.clone(),
            proxy: global.proxy.clone(),
            spoof_emulation: global.spoof_emulation.clone(),
            update_source: global.update_source.clone(),
            admin_key: global.admin_key.clone(),
            mask_sensitive_info: global.mask_sensitive_info,
            dsn: global.dsn.clone(),
            data_dir: global.data_dir.clone(),
        },
    ));

    storage
        .write_batch(batch)
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok(())
}

pub(super) async fn seed_registry_credentials_and_statuses(
    storage: &SeaOrmStorage,
    registry: &mut ProviderRegistry,
    provider_ids: &std::collections::HashMap<String, i64>,
) -> Result<()> {
    let existing_credentials = storage
        .list_credentials(&CredentialQuery {
            id: Scope::All,
            provider_id: Scope::All,
            kind: Scope::All,
            enabled: Scope::All,
            name_contains: None,
            limit: None,
            offset: None,
        })
        .await
        .context("list existing credentials for bootstrap seed")?;
    let mut credential_id_by_provider_and_name = existing_credentials
        .iter()
        .filter_map(|row| {
            row.name
                .as_ref()
                .map(|name| ((row.provider_id, name.clone()), row.id))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut next_credential_id = existing_credentials
        .iter()
        .map(|row| row.id)
        .max()
        .unwrap_or(-1)
        + 1;

    let existing_statuses = storage
        .list_credential_statuses(&CredentialStatusQuery {
            id: Scope::All,
            credential_id: Scope::All,
            provider_id: Scope::All,
            channel: Scope::All,
            health_kind: Scope::All,
            limit: None,
            offset: None,
        })
        .await
        .context("list existing credential statuses for bootstrap seed")?;
    let mut status_id_by_credential_and_channel = existing_statuses
        .iter()
        .map(|row| ((row.credential_id, row.channel.clone()), row.id))
        .collect::<std::collections::HashMap<_, _>>();
    let mut next_status_id = existing_statuses
        .iter()
        .map(|row| row.id)
        .max()
        .unwrap_or(-1)
        + 1;

    let mut batch = StorageWriteBatch::default();
    for provider in &mut registry.providers {
        let channel = provider.channel.as_str().to_string();
        let Some(provider_id) = provider_ids.get(channel.as_str()).copied() else {
            continue;
        };

        let mut runtime_to_db_credential_ids = std::collections::HashMap::new();
        for credential in provider.credentials.credentials.iter_mut() {
            let credential_name = credential
                .label
                .clone()
                .unwrap_or_else(|| credential.id.to_string());
            let credential_id = if let Some(id) = credential_id_by_provider_and_name
                .get(&(provider_id, credential_name.clone()))
                .copied()
            {
                id
            } else {
                let id = next_credential_id;
                next_credential_id += 1;
                credential_id_by_provider_and_name
                    .insert((provider_id, credential_name.clone()), id);
                id
            };

            runtime_to_db_credential_ids.insert(credential.id, credential_id);
            credential.id = credential_id;

            let secret_json = serde_json::to_string(&credential.credential)
                .context("serialize credential secret_json for bootstrap seed")?;
            batch.apply(StorageWriteEvent::UpsertCredential(CredentialWrite {
                id: credential_id,
                provider_id,
                name: Some(credential_name),
                kind: credential_kind_for_storage(&credential.credential),
                settings_json: None,
                secret_json,
                enabled: true,
            }));
        }

        for state in &mut provider.credentials.channel_states {
            if let Some(db_id) = runtime_to_db_credential_ids
                .get(&state.credential_id)
                .copied()
            {
                state.credential_id = db_id;
            }
        }

        let state_by_credential_id = provider
            .credentials
            .channel_states
            .iter()
            .map(|state| (state.credential_id, state))
            .collect::<std::collections::HashMap<_, _>>();

        for credential in provider.credentials.list_credentials() {
            let credential_id = credential.id;

            let (health_kind, health_json, checked_at_unix_ms, last_error) = state_by_credential_id
                .get(&credential_id)
                .map(|state| {
                    let (kind, json) = credential_health_to_storage(&state.health);
                    (
                        kind,
                        json,
                        state
                            .checked_at_unix_ms
                            .map(|value| value.min(i64::MAX as u64) as i64),
                        state.last_error.clone(),
                    )
                })
                .unwrap_or_else(|| ("healthy".to_string(), None, None, None));

            let status_id = if let Some(id) = status_id_by_credential_and_channel
                .get(&(credential_id, channel.clone()))
                .copied()
            {
                id
            } else {
                let id = next_status_id;
                next_status_id += 1;
                status_id_by_credential_and_channel.insert((credential_id, channel.clone()), id);
                id
            };

            batch.apply(StorageWriteEvent::UpsertCredentialStatus(
                CredentialStatusWrite {
                    id: Some(status_id),
                    credential_id,
                    channel: channel.clone(),
                    health_kind,
                    health_json,
                    checked_at_unix_ms,
                    last_error,
                },
            ));
        }
    }

    if !batch.is_empty() {
        storage
            .write_batch(batch)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    }

    hydrate_registry_credentials_and_statuses(storage, registry, provider_ids)
        .await
        .context("hydrate registry credentials and statuses from storage")?;

    Ok(())
}

pub(super) async fn hydrate_registry_credentials_and_statuses(
    storage: &SeaOrmStorage,
    registry: &mut ProviderRegistry,
    provider_ids: &std::collections::HashMap<String, i64>,
) -> Result<()> {
    let rows = storage
        .list_credentials(&CredentialQuery {
            id: Scope::All,
            provider_id: Scope::All,
            kind: Scope::All,
            enabled: Scope::Eq(true),
            name_contains: None,
            limit: None,
            offset: None,
        })
        .await
        .context("list credentials for runtime hydration")?;

    let statuses = storage
        .list_credential_statuses(&CredentialStatusQuery {
            id: Scope::All,
            credential_id: Scope::All,
            provider_id: Scope::All,
            channel: Scope::All,
            health_kind: Scope::All,
            limit: None,
            offset: None,
        })
        .await
        .context("list credential statuses for runtime hydration")?;
    let status_by_credential_and_channel = statuses
        .into_iter()
        .map(|item| ((item.credential_id, item.channel.clone()), item))
        .collect::<std::collections::HashMap<_, _>>();

    for provider in &mut registry.providers {
        let channel = provider.channel.as_str().to_string();
        let Some(provider_id) = provider_ids.get(channel.as_str()).copied() else {
            continue;
        };
        provider.credentials.credentials.clear();
        provider.credentials.channel_states.clear();

        for row in rows.iter().filter(|row| row.provider_id == provider_id) {
            let credential = serde_json::from_value::<ChannelCredential>(row.secret_json.clone())
                .with_context(|| {
                format!(
                    "deserialize credential secret_json id={} provider_id={}",
                    row.id, row.provider_id
                )
            })?;
            provider.credentials.upsert_credential(CredentialRef {
                id: row.id,
                label: row.name.clone(),
                credential,
            });

            if let Some(status) = status_by_credential_and_channel.get(&(row.id, channel.clone())) {
                let health = credential_health_from_storage(
                    status.health_kind.as_str(),
                    status.health_json.as_ref(),
                )
                .context("deserialize partial health model cooldown list")?;
                provider
                    .credentials
                    .upsert_channel_state(ChannelCredentialState {
                        channel: provider.channel.clone(),
                        credential_id: row.id,
                        health,
                        checked_at_unix_ms: status.checked_at.and_then(|checked| {
                            let unix_ms = checked.unix_timestamp_nanos() / 1_000_000;
                            if unix_ms < 0 {
                                return None;
                            }
                            u64::try_from(unix_ms).ok()
                        }),
                        last_error: status.last_error.clone(),
                    });
            }
        }
    }

    Ok(())
}
