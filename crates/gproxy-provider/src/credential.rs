use crate::channel::ChannelId;
use crate::channels::ChannelCredential;
use dashmap::DashMap;
use rand::prelude::IndexedRandom;
use serde::{Deserialize, Serialize};

pub(crate) fn normalize_model_cooldown_key(raw: &str) -> Option<String> {
    let mut value = raw.trim().trim_start_matches('/').trim();
    while let Some(stripped) = value.strip_prefix("models/") {
        value = stripped;
    }
    let value = value.trim().trim_start_matches('/').trim_end_matches('/');
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    pub id: i64,
    pub label: Option<String>,
    pub credential: ChannelCredential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CredentialHealthKind {
    Healthy,
    Partial,
    Dead,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCooldown {
    pub model: String,
    pub until_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialHealth {
    Healthy,
    /// Some models are temporarily unavailable for this credential.
    Partial {
        models: Vec<ModelCooldown>,
    },
    /// Entire credential is permanently unavailable.
    Dead,
}

impl CredentialHealth {
    pub const fn kind(&self) -> CredentialHealthKind {
        match self {
            Self::Healthy => CredentialHealthKind::Healthy,
            Self::Partial { .. } => CredentialHealthKind::Partial,
            Self::Dead => CredentialHealthKind::Dead,
        }
    }

    pub const fn is_healthy(&self) -> bool {
        matches!(self, Self::Healthy)
    }

    pub const fn is_partial(&self) -> bool {
        matches!(self, Self::Partial { .. })
    }

    pub const fn is_dead(&self) -> bool {
        matches!(self, Self::Dead)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelCredentialState {
    pub channel: ChannelId,
    pub credential_id: i64,
    pub health: CredentialHealth,
    pub checked_at_unix_ms: Option<u64>,
    pub last_error: Option<String>,
}

impl ChannelCredentialState {
    pub fn model_cooldown_until(&self, model: &str) -> Option<u64> {
        let CredentialHealth::Partial { models } = &self.health else {
            return None;
        };
        let normalized_model = normalize_model_cooldown_key(model)?;
        models
            .iter()
            .filter(|item| item.model == normalized_model)
            .map(|item| item.until_unix_ms)
            .max()
    }

    pub fn is_available_for(&self, model: Option<&str>, now_unix_ms: u64) -> bool {
        match &self.health {
            CredentialHealth::Healthy => true,
            CredentialHealth::Partial { models } => {
                let Some(model) = model else {
                    return true;
                };
                let Some(normalized_model) = normalize_model_cooldown_key(model) else {
                    return true;
                };
                !models
                    .iter()
                    .any(|item| item.model == normalized_model && item.until_unix_ms > now_unix_ms)
            }
            CredentialHealth::Dead => false,
        }
    }
}

pub type ChannelCredentialStateMap = DashMap<ChannelId, DashMap<i64, ChannelCredentialState>>;

#[derive(Debug, Default)]
pub struct ChannelCredentialStateStore {
    pub channel_states: ChannelCredentialStateMap,
}

impl ChannelCredentialStateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_states<I>(states: I) -> Self
    where
        I: IntoIterator<Item = ChannelCredentialState>,
    {
        let store = Self::new();
        for state in states {
            store.upsert(state);
        }
        store
    }

    pub fn upsert(&self, state: ChannelCredentialState) {
        self.channel_states
            .entry(state.channel.clone())
            .or_default()
            .insert(state.credential_id, state);
    }

    pub fn clear(&self) {
        self.channel_states.clear();
    }

    pub fn replace_from_states<I>(&self, states: I)
    where
        I: IntoIterator<Item = ChannelCredentialState>,
    {
        self.clear();
        for state in states {
            self.upsert(state);
        }
    }

    pub fn get(&self, channel: &ChannelId, credential_id: i64) -> Option<ChannelCredentialState> {
        self.channel_states
            .get(channel)
            .and_then(|states| states.get(&credential_id).map(|state| state.clone()))
    }

    pub fn remove(
        &self,
        channel: &ChannelId,
        credential_id: i64,
    ) -> Option<ChannelCredentialState> {
        let removed = self
            .channel_states
            .get(channel)
            .and_then(|states| states.remove(&credential_id).map(|(_, state)| state));

        let empty = self
            .channel_states
            .get(channel)
            .is_some_and(|states| states.is_empty());
        if empty {
            self.channel_states.remove(channel);
        }
        removed
    }

    pub fn eligible_credentials<'a>(
        &self,
        channel: &ChannelId,
        credentials: &'a [CredentialRef],
        model: Option<&str>,
        now_unix_ms: u64,
    ) -> Vec<&'a CredentialRef> {
        credentials
            .iter()
            .filter(|credential| {
                self.get(channel, credential.id)
                    .is_none_or(|state| state.is_available_for(model, now_unix_ms))
            })
            .collect()
    }

    pub fn pick_random_eligible_credential<'a>(
        &self,
        channel: &ChannelId,
        credentials: &'a [CredentialRef],
        model: Option<&str>,
        now_unix_ms: u64,
    ) -> Option<&'a CredentialRef> {
        let eligible = self.eligible_credentials(channel, credentials, model, now_unix_ms);
        let mut rng = rand::rng();
        eligible.choose(&mut rng).copied()
    }

    pub fn snapshot(&self) -> Vec<ChannelCredentialState> {
        let mut states = Vec::new();
        for by_channel in &self.channel_states {
            for state in by_channel.value() {
                states.push(state.value().clone());
            }
        }
        states
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderCredentialState {
    pub credentials: Vec<CredentialRef>,
    pub channel_states: Vec<ChannelCredentialState>,
}

impl ProviderCredentialState {
    pub fn to_state_store(&self) -> ChannelCredentialStateStore {
        ChannelCredentialStateStore::from_states(self.channel_states.iter().cloned())
    }

    pub fn list_credentials(&self) -> &[CredentialRef] {
        &self.credentials
    }

    pub fn credential(&self, credential_id: i64) -> Option<&CredentialRef> {
        self.credentials
            .iter()
            .find(|item| item.id == credential_id)
    }

    pub fn create_credential(&mut self, credential: CredentialRef) -> bool {
        if self.credentials.iter().any(|item| item.id == credential.id) {
            return false;
        }
        self.credentials.push(credential);
        true
    }

    pub fn update_credential(&mut self, credential: CredentialRef) -> bool {
        let Some(existing) = self
            .credentials
            .iter_mut()
            .find(|item| item.id == credential.id)
        else {
            return false;
        };
        *existing = credential;
        true
    }

    pub fn upsert_credential(&mut self, credential: CredentialRef) {
        if self.update_credential(credential.clone()) {
            return;
        }
        self.credentials.push(credential);
    }

    pub fn delete_credential(&mut self, credential_id: i64) -> Option<CredentialRef> {
        let index = self
            .credentials
            .iter()
            .position(|item| item.id == credential_id)?;
        let removed = self.credentials.remove(index);
        self.channel_states
            .retain(|item| item.credential_id != credential_id);
        Some(removed)
    }

    pub fn channel_state(
        &self,
        channel: &ChannelId,
        credential_id: i64,
    ) -> Option<&ChannelCredentialState> {
        self.channel_states
            .iter()
            .find(|item| &item.channel == channel && item.credential_id == credential_id)
    }

    pub fn upsert_channel_state(&mut self, state: ChannelCredentialState) {
        if let Some(existing) = self
            .channel_states
            .iter_mut()
            .find(|item| item.channel == state.channel && item.credential_id == state.credential_id)
        {
            *existing = state;
            return;
        }
        self.channel_states.push(state);
    }

    pub fn eligible_credentials<'a>(
        &'a self,
        channel: &ChannelId,
        model: Option<&str>,
        now_unix_ms: u64,
    ) -> Vec<&'a CredentialRef> {
        self.credentials
            .iter()
            .filter(|credential| {
                self.channel_state(channel, credential.id)
                    .is_none_or(|state| state.is_available_for(model, now_unix_ms))
            })
            .collect()
    }

    pub fn pick_random_eligible_credential<'a>(
        &'a self,
        channel: &ChannelId,
        model: Option<&str>,
        now_unix_ms: u64,
    ) -> Option<&'a CredentialRef> {
        let eligible = self.eligible_credentials(channel, model, now_unix_ms);
        let mut rng = rand::rng();
        eligible.choose(&mut rng).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChannelCredentialState, ChannelCredentialStateStore, CredentialHealth, CredentialRef,
        ModelCooldown, ProviderCredentialState,
    };
    use crate::channel::{BuiltinChannel, ChannelId};
    use crate::channels::{ChannelCredential, custom::CustomChannelCredential};

    #[test]
    fn partial_state_blocks_only_target_model_before_until() {
        let state = ChannelCredentialState {
            channel: ChannelId::Builtin(BuiltinChannel::Claude),
            credential_id: 1,
            health: CredentialHealth::Partial {
                models: vec![ModelCooldown {
                    model: "claude-4-sonnet".to_string(),
                    until_unix_ms: 2_000,
                }],
            },
            checked_at_unix_ms: None,
            last_error: None,
        };

        assert!(!state.is_available_for(Some("claude-4-sonnet"), 1_000));
        assert!(state.is_available_for(Some("claude-4-sonnet"), 2_001));
        assert!(state.is_available_for(Some("claude-3-5-haiku"), 1_000));
        assert!(state.is_available_for(None, 1_000));
    }

    #[test]
    fn dead_state_is_unavailable() {
        let state = ChannelCredentialState {
            channel: ChannelId::Builtin(BuiltinChannel::Claude),
            credential_id: 1,
            health: CredentialHealth::Dead,
            checked_at_unix_ms: None,
            last_error: None,
        };
        assert!(!state.is_available_for(Some("any"), 1_000));
        assert!(!state.is_available_for(None, 1_000));
    }

    #[test]
    fn pool_crud_and_model_aware_selection_work() {
        let channel = ChannelId::Builtin(BuiltinChannel::Claude);
        let mut pool = ProviderCredentialState::default();
        assert!(pool.create_credential(CredentialRef {
            id: 1,
            label: None,
            credential: ChannelCredential::Custom(CustomChannelCredential::default()),
        }));
        assert!(pool.create_credential(CredentialRef {
            id: 2,
            label: None,
            credential: ChannelCredential::Custom(CustomChannelCredential::default()),
        }));

        pool.upsert_channel_state(ChannelCredentialState {
            channel: channel.clone(),
            credential_id: 1,
            health: CredentialHealth::Partial {
                models: vec![ModelCooldown {
                    model: "claude-opus".to_string(),
                    until_unix_ms: 10_000,
                }],
            },
            checked_at_unix_ms: None,
            last_error: None,
        });
        pool.upsert_channel_state(ChannelCredentialState {
            channel: channel.clone(),
            credential_id: 2,
            health: CredentialHealth::Dead,
            checked_at_unix_ms: None,
            last_error: None,
        });

        let for_model = pool.eligible_credentials(&channel, Some("claude-opus"), 9_000);
        assert!(for_model.is_empty());

        let for_model_list = pool.eligible_credentials(&channel, None, 9_000);
        assert_eq!(for_model_list.len(), 1);
        assert_eq!(for_model_list[0].id, 1);

        let deleted = pool.delete_credential(1);
        assert_eq!(deleted.map(|item| item.id), Some(1));
        assert!(pool.credential(1).is_none());
        assert!(pool.channel_state(&channel, 1).is_none());
    }

    #[test]
    fn dashmap_store_model_list_and_model_level_selection() {
        let channel = ChannelId::Builtin(BuiltinChannel::Claude);
        let store = ChannelCredentialStateStore::new();
        let credentials = vec![
            CredentialRef {
                id: 1,
                label: None,
                credential: ChannelCredential::Custom(CustomChannelCredential::default()),
            },
            CredentialRef {
                id: 2,
                label: None,
                credential: ChannelCredential::Custom(CustomChannelCredential::default()),
            },
        ];

        store.upsert(ChannelCredentialState {
            channel: channel.clone(),
            credential_id: 1,
            health: CredentialHealth::Partial {
                models: vec![ModelCooldown {
                    model: "claude-opus".to_string(),
                    until_unix_ms: 10_000,
                }],
            },
            checked_at_unix_ms: None,
            last_error: None,
        });
        store.upsert(ChannelCredentialState {
            channel: channel.clone(),
            credential_id: 2,
            health: CredentialHealth::Dead,
            checked_at_unix_ms: None,
            last_error: None,
        });

        let model_list_eligible = store.eligible_credentials(&channel, &credentials, None, 1_000);
        assert_eq!(model_list_eligible.len(), 1);
        assert_eq!(model_list_eligible[0].id, 1);

        let model_eligible =
            store.eligible_credentials(&channel, &credentials, Some("claude-opus"), 1_000);
        assert!(model_eligible.is_empty());
    }
}
