use std::sync::Arc;

use gproxy_provider::{
    BuiltinChannelCredential, ChannelCredential, ChannelId, CredentialRef, ProviderDefinition,
    UpstreamCredentialUpdate, credential_kind_for_storage,
};
use gproxy_storage::{
    CredentialQuery, CredentialQueryRow, CredentialWrite, ProviderQuery, ProviderWrite, Scope,
    SeaOrmStorage, StorageWriteBatch, StorageWriteEvent, StorageWriteSink,
};

use crate::AppState;

use super::{HttpError, internal_error};

fn trimmed_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalized_email(value: Option<&str>) -> Option<String> {
    trimmed_non_empty(value).map(|value| value.to_ascii_lowercase())
}

fn combine_display_name(primary: Option<String>, qualifier: Option<String>) -> Option<String> {
    match (primary, qualifier) {
        (Some(primary), Some(qualifier)) if primary != qualifier => {
            Some(format!("{primary} ({qualifier})"))
        }
        (Some(primary), _) => Some(primary),
        (None, Some(qualifier)) => Some(qualifier),
        (None, None) => None,
    }
}

fn claudecode_qualifier(
    organization_uuid: Option<&str>,
    subscription_type: Option<&str>,
    rate_limit_tier: Option<&str>,
) -> Option<String> {
    if let Some(organization_uuid) = trimmed_non_empty(organization_uuid) {
        return Some(organization_uuid);
    }

    match (
        trimmed_non_empty(subscription_type),
        trimmed_non_empty(rate_limit_tier),
    ) {
        (Some(subscription_type), Some(rate_limit_tier)) => {
            Some(format!("{subscription_type} / {rate_limit_tier}"))
        }
        (Some(subscription_type), None) => Some(subscription_type),
        (None, Some(rate_limit_tier)) => Some(rate_limit_tier),
        (None, None) => None,
    }
}

fn credential_default_name(credential: &CredentialRef) -> String {
    trimmed_non_empty(credential.label.as_deref())
        .or_else(|| match &credential.credential {
            ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(value)) => {
                combine_display_name(
                    trimmed_non_empty(value.user_email.as_deref()),
                    claudecode_qualifier(
                        value.organization_uuid.as_deref(),
                        Some(value.subscription_type.as_str()),
                        Some(value.rate_limit_tier.as_str()),
                    ),
                )
            }
            ChannelCredential::Builtin(BuiltinChannelCredential::Codex(value)) => {
                combine_display_name(
                    trimmed_non_empty(value.user_email.as_deref()),
                    trimmed_non_empty(Some(value.account_id.as_str())),
                )
            }
            ChannelCredential::Builtin(BuiltinChannelCredential::GeminiCli(value)) => {
                trimmed_non_empty(value.user_email.as_deref())
            }
            ChannelCredential::Builtin(BuiltinChannelCredential::Antigravity(value)) => {
                trimmed_non_empty(value.user_email.as_deref())
            }
            ChannelCredential::Builtin(BuiltinChannelCredential::Vertex(value)) => {
                trimmed_non_empty(Some(value.client_email.as_str()))
            }
            _ => None,
        })
        .unwrap_or_else(|| credential.id.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CredentialIdentityKey {
    key: String,
    strong: bool,
}

fn codex_identity_key(
    account_id: Option<&str>,
    user_email: Option<&str>,
) -> Option<CredentialIdentityKey> {
    let account_id = trimmed_non_empty(account_id)?;
    let user_email = normalized_email(user_email).unwrap_or_default();
    Some(CredentialIdentityKey {
        key: format!("builtin/codex:{account_id}:{user_email}"),
        strong: true,
    })
}

fn claudecode_identity_key(
    organization_uuid: Option<&str>,
    user_email: Option<&str>,
    subscription_type: Option<&str>,
    rate_limit_tier: Option<&str>,
) -> Option<CredentialIdentityKey> {
    let user_email = normalized_email(user_email);

    if let Some(organization_uuid) = trimmed_non_empty(organization_uuid) {
        return Some(CredentialIdentityKey {
            key: format!(
                "builtin/claudecode:{organization_uuid}:{}",
                user_email.unwrap_or_default()
            ),
            strong: true,
        });
    }

    let subscription_type = trimmed_non_empty(subscription_type);
    let rate_limit_tier = trimmed_non_empty(rate_limit_tier);
    if let Some(user_email) = user_email
        && (subscription_type.is_some() || rate_limit_tier.is_some())
    {
        return Some(CredentialIdentityKey {
            key: format!(
                "builtin/claudecode:{user_email}:{}:{}",
                subscription_type.unwrap_or_default(),
                rate_limit_tier.unwrap_or_default()
            ),
            strong: false,
        });
    }

    None
}

fn credential_identity_key_from_channel_credential(
    credential: &ChannelCredential,
) -> Option<CredentialIdentityKey> {
    match credential {
        ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(value)) => {
            claudecode_identity_key(
                value.organization_uuid.as_deref(),
                value.user_email.as_deref(),
                Some(value.subscription_type.as_str()),
                Some(value.rate_limit_tier.as_str()),
            )
        }
        ChannelCredential::Builtin(BuiltinChannelCredential::Codex(value)) => {
            codex_identity_key(Some(value.account_id.as_str()), value.user_email.as_deref())
        }
        _ => None,
    }
}

fn credential_identity_key(credential: &CredentialRef) -> Option<CredentialIdentityKey> {
    credential_identity_key_from_channel_credential(&credential.credential)
}

fn row_credential_identity_key(row: &CredentialQueryRow) -> Option<CredentialIdentityKey> {
    let credential = serde_json::from_value::<ChannelCredential>(row.secret_json.clone()).ok()?;
    credential_identity_key_from_channel_credential(&credential)
}

async fn create_credential_row(
    storage: &SeaOrmStorage,
    provider_id: i64,
    expected_name: &str,
    credential: &CredentialRef,
) -> Result<i64, HttpError> {
    let credential_secret_json = serde_json::to_string(&credential.credential)
        .map_err(|err| internal_error(err.to_string()))?;
    storage
        .create_credential(
            provider_id,
            Some(expected_name),
            credential_kind_for_storage(&credential.credential).as_str(),
            None,
            credential_secret_json.as_str(),
            true,
        )
        .await
        .map_err(|err| internal_error(err.to_string()))
}

pub(super) async fn resolve_provider_id(
    state: &AppState,
    channel: &ChannelId,
) -> Result<i64, HttpError> {
    let storage = state.load_storage();
    let rows = storage
        .list_providers(&ProviderQuery {
            channel: Scope::Eq(channel.as_str().to_string()),
            name: Scope::All,
            enabled: Scope::All,
            limit: Some(1),
        })
        .await
        .map_err(|err| internal_error(err.to_string()))?;
    if let Some(row) = rows.into_iter().next() {
        return Ok(row.id);
    }

    let provider = state
        .load_config()
        .providers
        .get(channel)
        .cloned()
        .ok_or_else(|| {
            internal_error(format!("provider {} not found in config", channel.as_str()))
        })?;
    let provider_settings_json = gproxy_provider::provider_settings_to_json_string_with_routing(
        &provider.settings,
        provider.credential_pick_mode,
        provider.cache_affinity_max_keys,
    )
    .map_err(|err| internal_error(err.to_string()))?;
    let provider_dispatch_json =
        serde_json::to_string(&provider.dispatch).map_err(|err| internal_error(err.to_string()))?;
    storage
        .create_provider(
            channel.as_str(),
            channel.as_str(),
            provider_settings_json.as_str(),
            provider_dispatch_json.as_str(),
            true,
        )
        .await
        .map_err(|err| internal_error(err.to_string()))
}

pub(super) async fn resolve_credential_id(
    state: &AppState,
    provider_id: i64,
    credential: &CredentialRef,
) -> Result<i64, HttpError> {
    let storage = state.load_storage();
    let expected_name = credential_default_name(credential);
    let expected_identity = credential_identity_key(credential);
    let rows = storage
        .list_credentials(&CredentialQuery {
            id: Scope::All,
            provider_id: Scope::Eq(provider_id),
            kind: Scope::All,
            enabled: Scope::All,
            name_contains: None,
            limit: Some(256),
            offset: None,
        })
        .await
        .map_err(|err| internal_error(err.to_string()))?;

    if credential.id > 0
        && rows
            .iter()
            .any(|row| row.id == credential.id && row.provider_id == provider_id)
    {
        return Ok(credential.id);
    }

    if let Some(expected_identity) = expected_identity {
        if let Some(row) = rows.iter().find(|row| {
            row_credential_identity_key(row)
                .as_ref()
                .is_some_and(|identity| identity == &expected_identity)
        }) {
            return Ok(row.id);
        }

        if expected_identity.strong {
            return create_credential_row(
                storage.as_ref(),
                provider_id,
                expected_name.as_str(),
                credential,
            )
            .await;
        }
    }

    if let Some(row) = rows
        .into_iter()
        .find(|row| row.name.as_deref() == Some(expected_name.as_str()))
    {
        return Ok(row.id);
    }

    create_credential_row(
        storage.as_ref(),
        provider_id,
        expected_name.as_str(),
        credential,
    )
    .await
}

pub(super) async fn persist_provider_and_credential(
    state: &AppState,
    channel: &ChannelId,
    provider: &ProviderDefinition,
    credential: &CredentialRef,
) -> Result<(), HttpError> {
    let provider_id = resolve_provider_id(state, channel).await?;
    let provider_settings_json = gproxy_provider::provider_settings_to_json_string_with_routing(
        &provider.settings,
        provider.credential_pick_mode,
        provider.cache_affinity_max_keys,
    )
    .map_err(|err| internal_error(err.to_string()))?;
    let provider_dispatch_json =
        serde_json::to_string(&provider.dispatch).map_err(|err| internal_error(err.to_string()))?;
    let provider_write = ProviderWrite {
        id: provider_id,
        name: channel.as_str().to_string(),
        channel: channel.as_str().to_string(),
        settings_json: provider_settings_json,
        dispatch_json: provider_dispatch_json,
        enabled: true,
    };
    let credential_id = resolve_credential_id(state, provider_id, credential).await?;
    let credential_secret_json = serde_json::to_string(&credential.credential)
        .map_err(|err| internal_error(err.to_string()))?;
    let credential_write = CredentialWrite {
        id: credential_id,
        provider_id,
        name: Some(credential_default_name(credential)),
        kind: credential_kind_for_storage(&credential.credential),
        settings_json: None,
        secret_json: credential_secret_json,
        enabled: true,
    };
    let mut batch = StorageWriteBatch::default();
    batch.apply(StorageWriteEvent::UpsertProvider(provider_write));
    batch.apply(StorageWriteEvent::UpsertCredential(credential_write));
    state
        .load_storage()
        .write_batch(batch)
        .await
        .map_err(|err| internal_error(err.to_string()))
}

pub(super) async fn apply_credential_update_and_persist(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    update: UpstreamCredentialUpdate,
) {
    if !state.apply_upstream_credential_update_in_memory(&channel, &update) {
        eprintln!(
            "provider: skip credential update, in-memory apply failed channel={} credential_id={}",
            channel.as_str(),
            update.credential_id()
        );
        return;
    }
    let Some(credential) =
        state.get_provider_credential_in_memory(&channel, update.credential_id())
    else {
        eprintln!(
            "provider: skip credential update, updated credential missing in-memory channel={} credential_id={}",
            channel.as_str(),
            update.credential_id()
        );
        return;
    };

    if let Err(err) =
        persist_provider_and_credential(&state, &channel, &provider, &credential).await
    {
        eprintln!(
            "provider: persist credential update failed channel={} credential_id={} error={:?}",
            channel.as_str(),
            credential.id,
            err
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use gproxy_provider::{
        BuiltinChannel, BuiltinChannelCredential, BuiltinChannelSettings, ChannelCredential,
        ChannelId, ChannelSettings, CredentialPickMode, CredentialRef, LocalTokenizerStore,
        ProviderCredentialState, ProviderDefinition, ProviderDispatchTable, ProviderRegistry,
    };
    use gproxy_storage::{CredentialQuery, Scope, SeaOrmStorage, storage_write_channel};
    use wreq::Client as WreqClient;

    use crate::{AppStateInit, GlobalSettings};

    use super::{
        AppState, persist_provider_and_credential, resolve_credential_id, resolve_provider_id,
    };

    fn build_claudecode_provider() -> ProviderDefinition {
        ProviderDefinition {
            channel: ChannelId::Builtin(BuiltinChannel::ClaudeCode),
            dispatch: ProviderDispatchTable::default_for_builtin(BuiltinChannel::ClaudeCode),
            settings: ChannelSettings::Builtin(BuiltinChannelSettings::default_for(
                BuiltinChannel::ClaudeCode,
            )),
            credential_pick_mode: CredentialPickMode::RoundRobinNoCache,
            cache_affinity_max_keys: 0,
            credentials: ProviderCredentialState::default(),
        }
    }

    fn build_codex_provider() -> ProviderDefinition {
        ProviderDefinition {
            channel: ChannelId::Builtin(BuiltinChannel::Codex),
            dispatch: ProviderDispatchTable::default_for_builtin(BuiltinChannel::Codex),
            settings: ChannelSettings::Builtin(BuiltinChannelSettings::default_for(
                BuiltinChannel::Codex,
            )),
            credential_pick_mode: CredentialPickMode::RoundRobinNoCache,
            cache_affinity_max_keys: 0,
            credentials: ProviderCredentialState::default(),
        }
    }

    async fn build_state(provider: ProviderDefinition) -> Arc<AppState> {
        let storage = Arc::new(
            SeaOrmStorage::connect("sqlite::memory:", None)
                .await
                .expect("connect memory storage"),
        );
        storage.sync().await.expect("sync memory storage");

        let mut registry = ProviderRegistry::default();
        registry.upsert(provider);

        let (storage_writes, _storage_rx) = storage_write_channel(4);
        Arc::new(AppState::new(AppStateInit {
            storage,
            storage_writes,
            http: Arc::new(WreqClient::new()),
            spoof_http: Arc::new(WreqClient::new()),
            global: GlobalSettings::default(),
            providers: registry,
            tokenizers: Arc::new(LocalTokenizerStore::new(PathBuf::from("/tmp"))),
            users: Vec::new(),
            keys: HashMap::new(),
        }))
    }

    fn codex_credential(account_id: &str, user_email: &str) -> ChannelCredential {
        let mut credential = BuiltinChannelCredential::blank_for(BuiltinChannel::Codex);
        let BuiltinChannelCredential::Codex(value) = &mut credential else {
            unreachable!("blank codex credential should match codex variant");
        };
        value.access_token = "access".to_string();
        value.refresh_token = "refresh".to_string();
        value.id_token = "id".to_string();
        value.user_email = Some(user_email.to_string());
        value.account_id = account_id.to_string();
        ChannelCredential::Builtin(credential)
    }

    fn claudecode_credential(
        organization_uuid: Option<&str>,
        user_email: &str,
        subscription_type: &str,
        rate_limit_tier: &str,
    ) -> ChannelCredential {
        let mut credential = BuiltinChannelCredential::blank_for(BuiltinChannel::ClaudeCode);
        let BuiltinChannelCredential::ClaudeCode(value) = &mut credential else {
            unreachable!("blank claudecode credential should match claudecode variant");
        };
        value.access_token = "access".to_string();
        value.refresh_token = "refresh".to_string();
        value.enable_claude_1m_sonnet = Some(true);
        value.enable_claude_1m_opus = Some(true);
        value.subscription_type = subscription_type.to_string();
        value.rate_limit_tier = rate_limit_tier.to_string();
        value.user_email = Some(user_email.to_string());
        value.organization_uuid = organization_uuid.map(ToOwned::to_owned);
        ChannelCredential::Builtin(credential)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unlabeled_oauth_credential_reuses_created_row_instead_of_duplicating() {
        let provider = build_claudecode_provider();
        let channel = provider.channel.clone();
        let state = build_state(provider.clone()).await;

        let provider_id = resolve_provider_id(state.as_ref(), &channel)
            .await
            .expect("resolve provider id");

        let provisional = CredentialRef {
            id: -1,
            label: None,
            credential: ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(
                Default::default(),
            )),
        };
        let resolved_id = resolve_credential_id(state.as_ref(), provider_id, &provisional)
            .await
            .expect("resolve provisional credential id");

        let credential = CredentialRef {
            id: resolved_id,
            label: None,
            credential: provisional.credential.clone(),
        };
        persist_provider_and_credential(state.as_ref(), &channel, &provider, &credential)
            .await
            .expect("persist resolved credential");

        let rows = state
            .load_storage()
            .list_credentials(&CredentialQuery {
                id: Scope::All,
                provider_id: Scope::Eq(provider_id),
                kind: Scope::All,
                enabled: Scope::All,
                name_contains: None,
                limit: Some(16),
                offset: None,
            })
            .await
            .expect("list credentials");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, resolved_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unlabeled_oauth_credential_defaults_name_to_email() {
        let provider = build_claudecode_provider();
        let channel = provider.channel.clone();
        let state = build_state(provider.clone()).await;

        let mut builtin_credential =
            BuiltinChannelCredential::blank_for(BuiltinChannel::ClaudeCode);
        if let BuiltinChannelCredential::ClaudeCode(value) = &mut builtin_credential {
            value.user_email = Some("user@example.com".to_string());
        }
        let credential = CredentialRef {
            id: 42,
            label: None,
            credential: ChannelCredential::Builtin(builtin_credential),
        };
        persist_provider_and_credential(state.as_ref(), &channel, &provider, &credential)
            .await
            .expect("persist email named credential");

        let provider_id = resolve_provider_id(state.as_ref(), &channel)
            .await
            .expect("resolve provider id");
        let rows = state
            .load_storage()
            .list_credentials(&CredentialQuery {
                id: Scope::All,
                provider_id: Scope::Eq(provider_id),
                kind: Scope::All,
                enabled: Scope::All,
                name_contains: None,
                limit: Some(16),
                offset: None,
            })
            .await
            .expect("list credentials");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name.as_deref(), Some("user@example.com"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn codex_same_email_different_account_ids_create_distinct_rows() {
        let provider = build_codex_provider();
        let channel = provider.channel.clone();
        let state = build_state(provider.clone()).await;

        for account_id in ["acct_a", "acct_b"] {
            let credential = CredentialRef {
                id: -1,
                label: None,
                credential: codex_credential(account_id, "user@example.com"),
            };
            persist_provider_and_credential(state.as_ref(), &channel, &provider, &credential)
                .await
                .expect("persist codex credential");
        }

        let provider_id = resolve_provider_id(state.as_ref(), &channel)
            .await
            .expect("resolve provider id");
        let rows = state
            .load_storage()
            .list_credentials(&CredentialQuery {
                id: Scope::All,
                provider_id: Scope::Eq(provider_id),
                kind: Scope::All,
                enabled: Scope::All,
                name_contains: None,
                limit: Some(16),
                offset: None,
            })
            .await
            .expect("list credentials");

        assert_eq!(rows.len(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn claudecode_same_email_different_organization_ids_create_distinct_rows() {
        let provider = build_claudecode_provider();
        let channel = provider.channel.clone();
        let state = build_state(provider.clone()).await;

        for organization_uuid in ["org_a", "org_b"] {
            let credential = CredentialRef {
                id: -1,
                label: None,
                credential: claudecode_credential(
                    Some(organization_uuid),
                    "user@example.com",
                    "claude_pro",
                    "pro",
                ),
            };
            persist_provider_and_credential(state.as_ref(), &channel, &provider, &credential)
                .await
                .expect("persist claudecode credential");
        }

        let provider_id = resolve_provider_id(state.as_ref(), &channel)
            .await
            .expect("resolve provider id");
        let rows = state
            .load_storage()
            .list_credentials(&CredentialQuery {
                id: Scope::All,
                provider_id: Scope::Eq(provider_id),
                kind: Scope::All,
                enabled: Scope::All,
                name_contains: None,
                limit: Some(16),
                offset: None,
            })
            .await
            .expect("list credentials");

        assert_eq!(rows.len(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn claudecode_same_email_and_organization_reuses_existing_row() {
        let provider = build_claudecode_provider();
        let channel = provider.channel.clone();
        let state = build_state(provider.clone()).await;

        let credential = CredentialRef {
            id: -1,
            label: None,
            credential: claudecode_credential(
                Some("org_same"),
                "user@example.com",
                "claude_pro",
                "pro",
            ),
        };
        persist_provider_and_credential(state.as_ref(), &channel, &provider, &credential)
            .await
            .expect("persist first credential");

        let resolved_id = resolve_provider_id(state.as_ref(), &channel)
            .await
            .expect("resolve provider id");
        let rows = state
            .load_storage()
            .list_credentials(&CredentialQuery {
                id: Scope::All,
                provider_id: Scope::Eq(resolved_id),
                kind: Scope::All,
                enabled: Scope::All,
                name_contains: None,
                limit: Some(16),
                offset: None,
            })
            .await
            .expect("list credentials");
        let first_id = rows[0].id;

        let duplicate = CredentialRef {
            id: -1,
            label: None,
            credential: credential.credential.clone(),
        };
        let duplicate_id = resolve_credential_id(state.as_ref(), resolved_id, &duplicate)
            .await
            .expect("resolve duplicate credential");

        assert_eq!(duplicate_id, first_id);
    }
}
