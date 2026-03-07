use super::*;

impl AppState {
    pub fn upsert_credential_state(&self, state: gproxy_provider::ChannelCredentialState) {
        self.runtime.credential_states.upsert(state);
    }

    pub fn apply_upstream_credential_update_in_memory(
        &self,
        channel: &ChannelId,
        update: &UpstreamCredentialUpdate,
    ) -> bool {
        let mut snapshot = (*self.runtime.config.load_full()).clone();
        let applied = snapshot
            .providers
            .apply_upstream_credential_update(channel, update);
        if applied {
            self.runtime.config.store(Arc::new(snapshot));
        }
        applied
    }

    pub fn upsert_provider_in_memory(
        &self,
        channel: ChannelId,
        settings: ChannelSettings,
        dispatch: ProviderDispatchTable,
        credential_pick_mode: CredentialPickMode,
        enabled: bool,
    ) {
        let mut snapshot = (*self.runtime.config.load_full()).clone();
        if enabled {
            if let Some(existing) = snapshot.providers.get_mut(&channel) {
                existing.settings = settings;
                existing.dispatch = dispatch;
                existing.credential_pick_mode = credential_pick_mode;
            } else {
                snapshot.providers.upsert(ProviderDefinition {
                    channel: channel.clone(),
                    dispatch,
                    settings,
                    credential_pick_mode,
                    credentials: ProviderCredentialState::default(),
                });
            }
        } else {
            snapshot
                .providers
                .providers
                .retain(|item| item.channel != channel);
        }
        self.runtime.config.store(Arc::new(snapshot));
    }

    pub fn delete_provider_in_memory(&self, channel: &ChannelId) {
        let mut snapshot = (*self.runtime.config.load_full()).clone();
        snapshot
            .providers
            .providers
            .retain(|item| &item.channel != channel);
        self.runtime.config.store(Arc::new(snapshot));
    }

    pub fn upsert_provider_credential_in_memory(
        &self,
        channel: &ChannelId,
        credential: CredentialRef,
    ) -> bool {
        let mut snapshot = (*self.runtime.config.load_full()).clone();
        let applied = snapshot.providers.upsert_credential(channel, credential);
        if applied {
            self.runtime.config.store(Arc::new(snapshot));
        }
        applied
    }

    pub fn delete_provider_credential_in_memory(
        &self,
        channel: &ChannelId,
        credential_id: i64,
    ) -> bool {
        let mut snapshot = (*self.runtime.config.load_full()).clone();
        let Some(provider) = snapshot.providers.get_mut(channel) else {
            return false;
        };
        let removed = provider.delete_credential(credential_id).is_some();
        if removed {
            self.runtime
                .credential_states
                .remove(channel, credential_id);
            self.runtime.config.store(Arc::new(snapshot));
        }
        removed
    }

    pub fn get_provider_credential_in_memory(
        &self,
        channel: &ChannelId,
        credential_id: i64,
    ) -> Option<CredentialRef> {
        self.runtime
            .config
            .load()
            .providers
            .get(channel)
            .and_then(|provider| provider.credentials.credential(credential_id).cloned())
    }

    pub fn pick_random_eligible_credential(
        &self,
        channel: &ChannelId,
        model: Option<&str>,
        now_unix_ms: u64,
    ) -> Option<CredentialRef> {
        let config = self.runtime.config.load();
        pick_random_eligible_credential_from_snapshot(
            &config,
            &self.runtime.credential_states,
            channel,
            model,
            now_unix_ms,
        )
    }
}

pub(super) fn pick_random_eligible_credential_from_snapshot(
    config: &RuntimeConfigSnapshot,
    states: &ChannelCredentialStateStore,
    channel: &ChannelId,
    model: Option<&str>,
    now_unix_ms: u64,
) -> Option<CredentialRef> {
    let provider = config.providers.get(channel)?;
    let credential = states.pick_random_eligible_credential(
        channel,
        provider.credentials.list_credentials(),
        model,
        now_unix_ms,
    )?;
    Some(credential.clone())
}
