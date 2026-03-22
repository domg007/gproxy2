use super::storage_seed::{seed_registry_credentials_and_statuses, seed_registry_providers};
use super::*;
use gproxy_provider::{
    DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS,
    parse_credential_cache_affinity_max_keys_from_provider_settings_value,
};

pub(super) async fn build_seeded_provider_registry(
    storage: &SeaOrmStorage,
    config_for_providers: &BootstrapConfig,
) -> Result<ProviderRegistry> {
    let mut registry = build_provider_registry(config_for_providers);
    let (provider_ids, enabled_by_channel) =
        seed_registry_providers(storage, config_for_providers, &mut registry)
            .await
            .context("seed provider registry into storage")?;
    seed_registry_credentials_and_statuses(storage, &mut registry, &provider_ids)
        .await
        .context("seed registry credentials and statuses into storage")?;
    registry.providers.retain(|provider| {
        enabled_by_channel
            .get(provider.channel.as_str())
            .copied()
            .unwrap_or(false)
    });
    Ok(registry)
}

fn build_provider_registry(config: &BootstrapConfig) -> ProviderRegistry {
    let mut registry = ProviderRegistry::default();
    for channel_cfg in &config.channels {
        if channel_cfg.id.trim().is_empty() {
            eprintln!("bootstrap: skip channel with empty id");
            continue;
        }

        let channel = ChannelId::parse(channel_cfg.id.trim());
        let settings = resolve_provider_settings(&channel, &channel_cfg.settings);
        let credential_pick_mode =
            parse_credential_pick_mode_from_provider_settings_value(&channel_cfg.settings);
        let cache_affinity_max_keys =
            parse_credential_cache_affinity_max_keys_from_provider_settings_value(
                &channel_cfg.settings,
            )
            .unwrap_or_else(|err| {
                eprintln!(
                    "bootstrap: invalid cache affinity max keys for channel={} ({err}), fallback to default {}",
                    channel_cfg.id,
                    DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS
                );
                DEFAULT_CREDENTIAL_CACHE_AFFINITY_MAX_KEYS
            });
        let (credentials, channel_states) = dedupe_credentials(&channel, channel_cfg);

        let dispatch = channel_cfg
            .dispatch
            .clone()
            .unwrap_or_else(|| match &channel {
                ChannelId::Builtin(builtin) => ProviderDispatchTable::default_for_builtin(*builtin),
                ChannelId::Custom(_) => ProviderDispatchTable::default_for_custom(),
            });
        registry.upsert(ProviderDefinition {
            channel,
            dispatch,
            settings,
            credential_pick_mode,
            cache_affinity_max_keys,
            credentials: ProviderCredentialState {
                credentials,
                channel_states,
            },
        });
    }
    registry
}

pub(super) fn resolve_provider_settings(
    channel: &ChannelId,
    settings: &serde_json::Value,
) -> gproxy_provider::ChannelSettings {
    parse_provider_settings_value_for_channel(channel, settings).unwrap_or_else(|err| {
        eprintln!(
            "bootstrap: invalid settings for channel={} ({err}), fallback to default",
            channel.as_str()
        );
        parse_provider_settings_value_for_channel(channel, &serde_json::json!({}))
            .unwrap_or_default()
    })
}

fn dedupe_credentials(
    channel: &ChannelId,
    channel_cfg: &crate::bootstrap::config::ChannelConfigFile,
) -> (Vec<CredentialRef>, Vec<ChannelCredentialState>) {
    let mut credentials = Vec::new();
    let mut channel_states = Vec::new();

    let mut seen_ids = HashSet::<i64>::new();
    let mut seen_fingerprints = HashSet::new();

    for (idx, credential_cfg) in channel_cfg.credentials.iter().enumerate() {
        let id = credential_cfg
            .id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or((idx as i64) + 1);
        if !seen_ids.insert(id) {
            eprintln!(
                "bootstrap: drop duplicate credential id channel={} id={}",
                channel_cfg.id, id
            );
            continue;
        }

        let fingerprint = credential_fingerprint(credential_cfg);
        if !seen_fingerprints.insert(fingerprint) {
            eprintln!(
                "bootstrap: drop duplicate credential material channel={} id={}",
                channel_cfg.id, id
            );
            continue;
        }

        let Some(credential) = build_channel_credential(channel, credential_cfg) else {
            eprintln!(
                "bootstrap: skip unsupported credential payload channel={} id={}",
                channel_cfg.id, id
            );
            continue;
        };

        credentials.push(CredentialRef {
            id,
            label: credential_cfg.label.clone(),
            credential,
        });

        if let Some(state_cfg) = credential_cfg.state.as_ref() {
            channel_states.push(ChannelCredentialState {
                channel: channel.clone(),
                credential_id: id,
                health: credential_health_from_config(&state_cfg.health),
                checked_at_unix_ms: state_cfg.checked_at_unix_ms,
                last_error: state_cfg.last_error.clone(),
            });
        }
    }

    (credentials, channel_states)
}

fn build_channel_credential(
    channel: &ChannelId,
    credential: &CredentialConfigFile,
) -> Option<ChannelCredential> {
    if let Some(builtin) = credential.builtin.clone() {
        if matches!(channel, ChannelId::Builtin(_)) {
            return Some(ChannelCredential::Builtin(builtin));
        }
        return None;
    }

    let secret = credential
        .secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    gproxy_provider::credential_from_secret(channel, secret)
}

fn credential_fingerprint(credential: &CredentialConfigFile) -> String {
    if let Some(builtin) = credential.builtin.as_ref() {
        return format!(
            "builtin:{}",
            toml::to_string(builtin).unwrap_or_else(|_| "<serialize-error>".to_string())
        );
    }
    if let Some(secret) = credential.secret.as_ref() {
        return format!("secret:{}", secret.trim());
    }
    format!(
        "identity:{}:{}",
        credential.id.as_deref().map(str::trim).unwrap_or_default(),
        credential.label.as_deref().unwrap_or_default()
    )
}

fn credential_health_from_config(health: &CredentialHealthConfigFile) -> CredentialHealth {
    match health {
        CredentialHealthConfigFile::Healthy => CredentialHealth::Healthy,
        CredentialHealthConfigFile::Partial { models } => CredentialHealth::Partial {
            models: models.clone(),
        },
        CredentialHealthConfigFile::Dead => CredentialHealth::Dead,
    }
}

#[cfg(test)]
mod tests {
    use gproxy_provider::{BuiltinChannel, ProviderDispatchTable};
    use gproxy_storage::{CredentialQuery, ProviderQuery, Scope, SeaOrmStorage};

    use super::*;
    use crate::bootstrap::config::{BootstrapConfig, ChannelConfigFile, CredentialConfigFile};

    async fn memory_storage() -> SeaOrmStorage {
        let storage = SeaOrmStorage::connect("sqlite::memory:", None)
            .await
            .expect("connect memory storage");
        storage.sync().await.expect("sync memory storage");
        storage
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disabled_config_channel_is_persisted_but_not_loaded() {
        let storage = memory_storage().await;
        let registry = build_seeded_provider_registry(
            &storage,
            &BootstrapConfig {
                channels: vec![ChannelConfigFile {
                    id: "openai".to_string(),
                    enabled: false,
                    settings: serde_json::json!({}),
                    dispatch: None,
                    credentials: vec![CredentialConfigFile {
                        id: None,
                        label: Some("primary".to_string()),
                        secret: Some("sk-test".to_string()),
                        builtin: None,
                        state: None,
                    }],
                }],
                ..BootstrapConfig::default()
            },
        )
        .await
        .expect("build seeded provider registry");

        assert!(registry.get(&ChannelId::parse("openai")).is_none());

        let providers = storage
            .list_providers(&ProviderQuery {
                channel: Scope::Eq("openai".to_string()),
                name: Scope::All,
                enabled: Scope::All,
                limit: None,
            })
            .await
            .expect("query providers");
        assert_eq!(providers.len(), 1);
        assert!(!providers[0].enabled);

        let credentials = storage
            .list_credentials(&CredentialQuery {
                id: Scope::All,
                provider_id: Scope::Eq(providers[0].id),
                kind: Scope::All,
                enabled: Scope::All,
                name_contains: None,
                limit: None,
                offset: None,
            })
            .await
            .expect("query credentials");
        assert_eq!(credentials.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disabled_storage_channel_stays_disabled_after_bootstrap() {
        let storage = memory_storage().await;
        let dispatch_json = serde_json::to_string(&ProviderDispatchTable::default_for_builtin(
            BuiltinChannel::OpenAi,
        ))
        .expect("serialize dispatch");
        storage
            .create_provider("openai", "openai", "{}", dispatch_json.as_str(), false)
            .await
            .expect("create provider");

        let registry = build_seeded_provider_registry(&storage, &BootstrapConfig::default())
            .await
            .expect("build seeded provider registry");

        assert!(registry.get(&ChannelId::parse("openai")).is_none());

        let providers = storage
            .list_providers(&ProviderQuery {
                channel: Scope::Eq("openai".to_string()),
                name: Scope::All,
                enabled: Scope::All,
                limit: None,
            })
            .await
            .expect("query providers");
        assert_eq!(providers.len(), 1);
        assert!(!providers[0].enabled);
    }
}
