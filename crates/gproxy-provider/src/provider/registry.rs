use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderRegistry {
    pub providers: Vec<ProviderDefinition>,
}

impl ProviderRegistry {
    pub fn get(&self, channel: &ChannelId) -> Option<&ProviderDefinition> {
        self.providers
            .iter()
            .find(|provider| &provider.channel == channel)
    }

    pub fn get_mut(&mut self, channel: &ChannelId) -> Option<&mut ProviderDefinition> {
        self.providers
            .iter_mut()
            .find(|provider| &provider.channel == channel)
    }

    pub fn upsert(&mut self, provider: ProviderDefinition) {
        if let Some(existing) = self
            .providers
            .iter_mut()
            .find(|item| item.channel == provider.channel)
        {
            *existing = provider;
            return;
        }
        self.providers.push(provider);
    }

    pub fn create_credential(&mut self, channel: &ChannelId, credential: CredentialRef) -> bool {
        let Some(provider) = self.get_mut(channel) else {
            return false;
        };
        provider.create_credential(credential)
    }

    pub fn update_credential(&mut self, channel: &ChannelId, credential: CredentialRef) -> bool {
        let Some(provider) = self.get_mut(channel) else {
            return false;
        };
        provider.update_credential(credential)
    }

    pub fn upsert_credential(&mut self, channel: &ChannelId, credential: CredentialRef) -> bool {
        let Some(provider) = self.get_mut(channel) else {
            return false;
        };
        provider.upsert_credential(credential);
        true
    }

    pub fn delete_credential(
        &mut self,
        channel: &ChannelId,
        credential_id: i64,
    ) -> Option<CredentialRef> {
        let provider = self.get_mut(channel)?;
        provider.delete_credential(credential_id)
    }

    pub fn pick_random_eligible_credential(
        &self,
        channel: &ChannelId,
        model: Option<&str>,
        now_unix_ms: u64,
    ) -> Option<&CredentialRef> {
        self.get(channel)
            .and_then(|provider| provider.pick_random_eligible_credential(model, now_unix_ms))
    }

    pub fn apply_upstream_credential_update(
        &mut self,
        channel: &ChannelId,
        update: &UpstreamCredentialUpdate,
    ) -> bool {
        let Some(provider) = self.get_mut(channel) else {
            return false;
        };
        provider.apply_upstream_credential_update(update)
    }
}
