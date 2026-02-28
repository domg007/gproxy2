use crate::credential::normalize_model_cooldown_key;
use crate::{
    ChannelCredentialState, ChannelCredentialStateStore, ChannelId, CredentialHealth, ModelCooldown,
};

pub const DEFAULT_RATE_LIMIT_COOLDOWN_MS: u64 = 60_000;
pub const DEFAULT_TRANSIENT_COOLDOWN_MS: u64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialStateManager {
    pub now_unix_ms: u64,
    pub default_rate_limit_cooldown_ms: u64,
    pub default_transient_cooldown_ms: u64,
}

impl CredentialStateManager {
    pub const fn new(now_unix_ms: u64) -> Self {
        Self {
            now_unix_ms,
            default_rate_limit_cooldown_ms: DEFAULT_RATE_LIMIT_COOLDOWN_MS,
            default_transient_cooldown_ms: DEFAULT_TRANSIENT_COOLDOWN_MS,
        }
    }

    pub fn mark_success(
        &self,
        states: &ChannelCredentialStateStore,
        channel: &ChannelId,
        credential_id: i64,
    ) {
        states.upsert(ChannelCredentialState {
            channel: channel.clone(),
            credential_id,
            health: CredentialHealth::Healthy,
            checked_at_unix_ms: Some(self.now_unix_ms),
            last_error: None,
        });
    }

    pub fn mark_auth_dead(
        &self,
        states: &ChannelCredentialStateStore,
        channel: &ChannelId,
        credential_id: i64,
        last_error: Option<String>,
    ) {
        states.upsert(ChannelCredentialState {
            channel: channel.clone(),
            credential_id,
            health: CredentialHealth::Dead,
            checked_at_unix_ms: Some(self.now_unix_ms),
            last_error,
        });
    }

    pub fn mark_rate_limited(
        &self,
        states: &ChannelCredentialStateStore,
        channel: &ChannelId,
        credential_id: i64,
        model: Option<&str>,
        cooldown_ms: Option<u64>,
        last_error: Option<String>,
    ) {
        let until = self
            .now_unix_ms
            .saturating_add(cooldown_ms.unwrap_or(self.default_rate_limit_cooldown_ms));
        self.mark_partial_with_model_cooldown(
            states,
            channel,
            credential_id,
            model,
            until,
            last_error,
        );
    }

    pub fn mark_transient_failure(
        &self,
        states: &ChannelCredentialStateStore,
        channel: &ChannelId,
        credential_id: i64,
        model: Option<&str>,
        cooldown_ms: Option<u64>,
        last_error: Option<String>,
    ) {
        let until = self
            .now_unix_ms
            .saturating_add(cooldown_ms.unwrap_or(self.default_transient_cooldown_ms));
        self.mark_partial_with_model_cooldown(
            states,
            channel,
            credential_id,
            model,
            until,
            last_error,
        );
    }

    fn mark_partial_with_model_cooldown(
        &self,
        states: &ChannelCredentialStateStore,
        channel: &ChannelId,
        credential_id: i64,
        model: Option<&str>,
        until_unix_ms: u64,
        last_error: Option<String>,
    ) {
        let Some(model) = model.and_then(normalize_model_cooldown_key) else {
            let health = states
                .get(channel, credential_id)
                .map(|state| state.health)
                .unwrap_or(CredentialHealth::Healthy);
            states.upsert(ChannelCredentialState {
                channel: channel.clone(),
                credential_id,
                health,
                checked_at_unix_ms: Some(self.now_unix_ms),
                last_error,
            });
            return;
        };

        let current = states.get(channel, credential_id);
        let mut models = match current.as_ref().map(|state| &state.health) {
            Some(CredentialHealth::Partial { models }) => models
                .iter()
                .filter(|item| item.until_unix_ms > self.now_unix_ms)
                .cloned()
                .collect::<Vec<_>>(),
            Some(CredentialHealth::Dead) => {
                states.upsert(ChannelCredentialState {
                    channel: channel.clone(),
                    credential_id,
                    health: CredentialHealth::Dead,
                    checked_at_unix_ms: Some(self.now_unix_ms),
                    last_error,
                });
                return;
            }
            _ => Vec::new(),
        };

        if let Some(item) = models.iter_mut().find(|item| item.model == model) {
            item.until_unix_ms = item.until_unix_ms.max(until_unix_ms);
        } else {
            models.push(ModelCooldown {
                model,
                until_unix_ms,
            });
        }

        states.upsert(ChannelCredentialState {
            channel: channel.clone(),
            credential_id,
            health: CredentialHealth::Partial { models },
            checked_at_unix_ms: Some(self.now_unix_ms),
            last_error,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::CredentialStateManager;
    use crate::{
        BuiltinChannel, ChannelCredentialStateStore, ChannelId, CredentialHealth, ModelCooldown,
    };

    #[test]
    fn auth_dead_marks_credential_dead() {
        let manager = CredentialStateManager::new(1_000);
        let store = ChannelCredentialStateStore::new();
        let channel = ChannelId::Builtin(BuiltinChannel::OpenAi);
        manager.mark_auth_dead(&store, &channel, 1, Some("401 unauthorized".to_string()));
        let state = store.get(&channel, 1).expect("state exists");
        assert!(matches!(state.health, CredentialHealth::Dead));
        assert_eq!(state.checked_at_unix_ms, Some(1_000));
    }

    #[test]
    fn rate_limit_updates_model_cooldown() {
        let manager = CredentialStateManager::new(10_000);
        let store = ChannelCredentialStateStore::new();
        let channel = ChannelId::Builtin(BuiltinChannel::OpenAi);

        manager.mark_rate_limited(&store, &channel, 1, Some("gpt-4.1"), Some(5_000), None);
        manager.mark_rate_limited(&store, &channel, 1, Some("gpt-4.1"), Some(2_000), None);

        let state = store.get(&channel, 1).expect("state exists");
        let CredentialHealth::Partial { models } = state.health else {
            panic!("expected partial");
        };
        assert_eq!(models.len(), 1);
        assert_eq!(
            models[0],
            ModelCooldown {
                model: "gpt-4.1".to_string(),
                until_unix_ms: 15_000
            }
        );
    }

    #[test]
    fn success_resets_to_healthy() {
        let manager = CredentialStateManager::new(20_000);
        let store = ChannelCredentialStateStore::new();
        let channel = ChannelId::Builtin(BuiltinChannel::OpenAi);

        manager.mark_rate_limited(
            &store,
            &channel,
            1,
            Some("gpt-4.1"),
            Some(5_000),
            Some("rate-limited".to_string()),
        );
        manager.mark_success(&store, &channel, 1);

        let state = store.get(&channel, 1).expect("state exists");
        assert!(matches!(state.health, CredentialHealth::Healthy));
        assert_eq!(state.last_error, None);
        assert_eq!(state.checked_at_unix_ms, Some(20_000));
    }
}
