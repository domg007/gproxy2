use std::sync::Arc;

use gproxy_provider::{
    ChannelId, CredentialRef, ProviderDefinition, UpstreamCredentialUpdate,
    credential_kind_for_storage,
};
use gproxy_storage::{
    CredentialQuery, CredentialWrite, ProviderQuery, ProviderWrite, Scope, StorageWriteBatch,
    StorageWriteEvent, StorageWriteSink,
};

use crate::AppState;

use super::{HttpError, internal_error};

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
    let expected_name = credential
        .label
        .clone()
        .unwrap_or_else(|| credential.id.to_string());
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

    if let Some(row) = rows
        .into_iter()
        .find(|row| row.name.as_deref() == Some(expected_name.as_str()))
    {
        return Ok(row.id);
    }

    let credential_secret_json = serde_json::to_string(&credential.credential)
        .map_err(|err| internal_error(err.to_string()))?;
    storage
        .create_credential(
            provider_id,
            Some(expected_name.as_str()),
            credential_kind_for_storage(&credential.credential).as_str(),
            None,
            credential_secret_json.as_str(),
            true,
        )
        .await
        .map_err(|err| internal_error(err.to_string()))
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
        name: credential
            .label
            .clone()
            .or_else(|| Some(credential.id.to_string())),
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
}
