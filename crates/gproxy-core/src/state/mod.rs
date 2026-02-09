use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use arc_swap::ArcSwap;
use time::OffsetDateTime;

use gproxy_common::GlobalConfig;
use gproxy_common::GlobalConfigPatch;
use gproxy_provider_core::{Credential, CredentialPool, EventHub};
use gproxy_storage::{CredentialRow, ProviderRow, StorageSnapshot, UserKeyRow, UserRow};

pub struct ProviderRuntime {
    pub provider_id: String,
    /// Provider config as JSON for now (parsed into typed ProviderConfig later).
    pub config_json: ArcSwap<serde_json::Value>,
    pub pool: CredentialPool,
}

pub struct AppState {
    pub global: ArcSwap<GlobalConfig>,
    pub providers: ArcSwap<HashMap<String, Arc<ProviderRuntime>>>,
    pub snapshot: ArcSwap<StorageSnapshot>,
    pub events: EventHub,
}

pub struct CredentialInsertInput {
    pub id: i64,
    pub provider_name: String,
    pub provider_id: i64,
    pub name: Option<String>,
    pub settings_json: serde_json::Value,
    pub secret_json: serde_json::Value,
    pub enabled: bool,
}

impl AppState {
    pub async fn from_bootstrap(
        global: GlobalConfig,
        snapshot: StorageSnapshot,
        events: EventHub,
    ) -> anyhow::Result<Self> {
        let mut providers: HashMap<String, Arc<ProviderRuntime>> = HashMap::new();
        let mut provider_id_to_name: HashMap<i64, String> = HashMap::new();

        // Create per-provider runtimes first.
        for p in &snapshot.providers {
            provider_id_to_name.insert(p.id, p.name.clone());
            let runtime = ProviderRuntime {
                provider_id: p.name.clone(),
                config_json: ArcSwap::from_pointee(p.config_json.clone()),
                pool: CredentialPool::new(events.clone()),
            };
            providers.insert(p.name.clone(), Arc::new(runtime));
        }

        // Load credentials into the corresponding provider pool (in-memory only).
        for c in &snapshot.credentials {
            if !c.enabled {
                continue;
            }
            let Some(provider_name) = provider_id_to_name.get(&c.provider_id) else {
                continue;
            };
            let Some(runtime) = providers.get(provider_name) else {
                continue;
            };
            let cred: Credential = serde_json::from_value(c.secret_json.clone())
                .with_context(|| format!("decode credential_json for credential_id={}", c.id))?;
            runtime.pool.insert(provider_name.clone(), c.id, cred).await;
        }

        Ok(Self {
            global: ArcSwap::from_pointee(global),
            providers: ArcSwap::from_pointee(providers),
            snapshot: ArcSwap::from_pointee(snapshot),
            events,
        })
    }

    pub fn apply_global_config(&self, config: GlobalConfig) {
        self.global.store(Arc::new(config));
    }

    pub fn apply_provider_upsert(
        &self,
        id: i64,
        name: String,
        config_json: serde_json::Value,
        enabled: bool,
    ) {
        let now = OffsetDateTime::now_utc();

        // 1) Update snapshot (admin/proxy reads only).
        let mut snap = self.snapshot.load().as_ref().clone();
        match snap.providers.iter_mut().find(|p| p.name == name) {
            Some(p) => {
                p.id = id;
                p.config_json = config_json.clone();
                p.enabled = enabled;
                p.updated_at = now;
            }
            None => snap.providers.push(ProviderRow {
                id,
                name: name.clone(),
                config_json: config_json.clone(),
                enabled,
                updated_at: now,
            }),
        }
        self.snapshot.store(Arc::new(snap));

        // 2) Ensure a runtime exists (used by proxy engine for upstream IO).
        let mut map = self.providers.load().as_ref().clone();
        match map.get(&name) {
            Some(rt) => rt.config_json.store(Arc::new(config_json)),
            None => {
                map.insert(
                    name.clone(),
                    Arc::new(ProviderRuntime {
                        provider_id: name.clone(),
                        config_json: ArcSwap::from_pointee(config_json),
                        pool: CredentialPool::new(self.events.clone()),
                    }),
                );
                self.providers.store(Arc::new(map));
            }
        }
    }

    pub fn apply_provider_delete(&self, name: &str) {
        // Remove from snapshot (including credentials that belonged to the provider).
        let mut snap = self.snapshot.load().as_ref().clone();
        let provider_id = snap.providers.iter().find(|p| p.name == name).map(|p| p.id);
        snap.providers.retain(|p| p.name != name);
        if let Some(pid) = provider_id {
            snap.credentials.retain(|c| c.provider_id != pid);
        }
        self.snapshot.store(Arc::new(snap));

        // Remove runtime.
        let mut map = self.providers.load().as_ref().clone();
        map.remove(name);
        self.providers.store(Arc::new(map));
    }

    pub fn apply_credential_delete(&self, credential_id: i64) {
        let mut snap = self.snapshot.load().as_ref().clone();
        snap.credentials.retain(|c| c.id != credential_id);
        self.snapshot.store(Arc::new(snap));
        // Pool removal is handled by disabling (set_enabled=false); for delete we currently
        // just remove from the provider index by best-effort.
        // If needed, we can add a pool.delete(id) later.
    }

    pub async fn apply_credential_update(
        &self,
        credential_id: i64,
        name: Option<String>,
        settings_json: serde_json::Value,
        secret_json: serde_json::Value,
    ) -> anyhow::Result<()> {
        let now = OffsetDateTime::now_utc();

        // Update snapshot and find provider.
        let mut snap = self.snapshot.load().as_ref().clone();
        let Some(row) = snap.credentials.iter_mut().find(|c| c.id == credential_id) else {
            return Ok(());
        };
        row.name = name.clone();
        row.settings_json = settings_json;
        row.secret_json = secret_json.clone();
        row.updated_at = now;
        let provider_name = snap
            .providers
            .iter()
            .find(|p| p.id == row.provider_id)
            .map(|p| p.name.clone());
        let enabled = row.enabled;
        self.snapshot.store(Arc::new(snap));

        // If enabled, ensure pool has the latest credential material.
        if enabled {
            let Some(provider_name) = provider_name else {
                return Ok(());
            };
            let Some(runtime) = self.providers.load().get(&provider_name).cloned() else {
                return Ok(());
            };
            let cred: Credential = serde_json::from_value(secret_json).with_context(|| {
                format!("decode credential_json for credential_id={credential_id} provider={provider_name}")
            })?;
            runtime
                .pool
                .insert(provider_name.clone(), credential_id, cred)
                .await;
        }
        Ok(())
    }

    pub fn apply_global_config_patch(
        &self,
        patch: GlobalConfigPatch,
    ) -> anyhow::Result<GlobalConfig> {
        let current = self.global.load().as_ref().clone();
        let mut merged = GlobalConfigPatch::from(current);
        merged.overlay(patch);
        let next = merged.into_config()?;
        self.global.store(Arc::new(next.clone()));
        Ok(next)
    }

    pub async fn apply_credential_insert(
        &self,
        input: CredentialInsertInput,
    ) -> anyhow::Result<()> {
        let now = OffsetDateTime::now_utc();
        let CredentialInsertInput {
            id,
            provider_name,
            provider_id,
            name,
            settings_json,
            secret_json,
            enabled,
        } = input;

        // Update snapshot first.
        let mut snap = self.snapshot.load().as_ref().clone();
        snap.credentials.push(CredentialRow {
            id,
            provider_id,
            name,
            settings_json,
            secret_json: secret_json.clone(),
            enabled,
            created_at: now,
            updated_at: now,
        });
        self.snapshot.store(Arc::new(snap));

        // Update pool (enabled credentials only).
        if enabled {
            let Some(runtime) = self.providers.load().get(&provider_name).cloned() else {
                return Ok(());
            };
            let cred: Credential = serde_json::from_value(secret_json).with_context(|| {
                format!("decode credential_json for credential_id={id} provider={provider_name}")
            })?;
            runtime.pool.insert(provider_name, id, cred).await;
        }
        Ok(())
    }

    pub async fn apply_credential_enabled(
        &self,
        credential_id: i64,
        enabled: bool,
    ) -> anyhow::Result<()> {
        let now = OffsetDateTime::now_utc();

        let mut snap = self.snapshot.load().as_ref().clone();
        let Some(row) = snap.credentials.iter_mut().find(|c| c.id == credential_id) else {
            // Unknown in memory; nothing to do.
            return Ok(());
        };
        row.enabled = enabled;
        row.updated_at = now;

        // Resolve provider name for pool operation.
        let provider_name = snap
            .providers
            .iter()
            .find(|p| p.id == row.provider_id)
            .map(|p| p.name.clone());
        let secret_json = row.secret_json.clone();

        self.snapshot.store(Arc::new(snap));

        let Some(provider_name) = provider_name else {
            return Ok(());
        };
        let Some(runtime) = self.providers.load().get(&provider_name).cloned() else {
            return Ok(());
        };

        if enabled {
            // Ensure the credential exists in the pool (even if it was disabled at bootstrap).
            let cred: Credential = serde_json::from_value(secret_json).with_context(|| {
                format!("decode credential_json for credential_id={credential_id} provider={provider_name}")
            })?;
            runtime
                .pool
                .insert(provider_name.clone(), credential_id, cred)
                .await;
            runtime
                .pool
                .set_enabled(&provider_name, credential_id, true)
                .await;
        } else {
            runtime
                .pool
                .set_enabled(&provider_name, credential_id, false)
                .await;
        }

        Ok(())
    }

    pub fn apply_user_upsert(&self, id: i64, name: String, enabled: bool) {
        let now = OffsetDateTime::now_utc();

        let mut snap = self.snapshot.load().as_ref().clone();
        match snap.users.iter_mut().find(|u| u.id == id) {
            Some(u) => {
                u.id = id;
                u.name = name;
                u.enabled = enabled;
                u.updated_at = now;
            }
            None => snap.users.push(UserRow {
                id,
                name,
                enabled,
                created_at: now,
                updated_at: now,
            }),
        }
        self.snapshot.store(Arc::new(snap));
    }

    pub fn apply_user_enabled(&self, user_id: i64, enabled: bool) {
        let now = OffsetDateTime::now_utc();

        let mut snap = self.snapshot.load().as_ref().clone();
        if let Some(u) = snap.users.iter_mut().find(|u| u.id == user_id) {
            u.enabled = enabled;
            u.updated_at = now;
            self.snapshot.store(Arc::new(snap));
        }
    }

    pub fn apply_user_delete(&self, user_id: i64) {
        let mut snap = self.snapshot.load().as_ref().clone();
        snap.users.retain(|u| u.id != user_id);
        snap.user_keys.retain(|k| k.user_id != user_id);
        self.snapshot.store(Arc::new(snap));
    }

    pub fn apply_user_key_insert(
        &self,
        id: i64,
        user_id: i64,
        api_key: String,
        label: Option<String>,
        enabled: bool,
    ) {
        let now = OffsetDateTime::now_utc();

        let mut snap = self.snapshot.load().as_ref().clone();
        snap.user_keys.push(UserKeyRow {
            id,
            user_id,
            api_key,
            label,
            enabled,
            created_at: now,
            updated_at: now,
        });
        self.snapshot.store(Arc::new(snap));
    }

    pub fn apply_user_key_label(&self, user_key_id: i64, label: Option<String>) {
        let now = OffsetDateTime::now_utc();

        let mut snap = self.snapshot.load().as_ref().clone();
        if let Some(k) = snap.user_keys.iter_mut().find(|k| k.id == user_key_id) {
            k.label = label;
            k.updated_at = now;
            self.snapshot.store(Arc::new(snap));
        }
    }

    pub fn apply_user_key_delete(&self, user_key_id: i64) {
        let mut snap = self.snapshot.load().as_ref().clone();
        snap.user_keys.retain(|k| k.id != user_key_id);
        self.snapshot.store(Arc::new(snap));
    }

    pub fn apply_user_key_enabled(&self, user_key_id: i64, enabled: bool) {
        let now = OffsetDateTime::now_utc();

        let mut snap = self.snapshot.load().as_ref().clone();
        if let Some(k) = snap.user_keys.iter_mut().find(|k| k.id == user_key_id) {
            k.enabled = enabled;
            k.updated_at = now;
            self.snapshot.store(Arc::new(snap));
        }
    }
}
