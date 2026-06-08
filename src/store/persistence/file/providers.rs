//! File-backend provider ops over `providers.json`.

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{Provider, ProviderInput};

use super::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("providers.json")
}

pub(super) async fn list(root: &Path) -> anyhow::Result<Vec<Provider>> {
    Ok(table::load::<Provider>(&path(root)).await?.rows)
}

pub(super) async fn get(root: &Path, id: i64) -> anyhow::Result<Option<Provider>> {
    Ok(table::load::<Provider>(&path(root))
        .await?
        .rows
        .into_iter()
        .find(|p| p.id == id))
}

pub(super) async fn get_by_name(root: &Path, name: &str) -> anyhow::Result<Option<Provider>> {
    Ok(table::load::<Provider>(&path(root))
        .await?
        .rows
        .into_iter()
        .find(|p| p.name == name))
}

pub(super) async fn upsert(root: &Path, input: ProviderInput) -> anyhow::Result<Provider> {
    let file = path(root);
    let mut t = table::load::<Provider>(&file).await?;
    let now = now_secs();

    // Reject name collisions with a different row.
    if let Some(existing) = t.rows.iter().find(|p| p.name == input.name)
        && Some(existing.id) != input.id
    {
        anyhow::bail!("provider name already exists: {}", input.name);
    }

    let stored = match input.id {
        Some(id) => {
            let row = t
                .rows
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or_else(|| anyhow::anyhow!("provider not found: {id}"))?;
            row.name = input.name;
            row.channel = input.channel;
            row.label = input.label;
            row.settings_json = input.settings_json;
            row.credential_strategy = input.credential_strategy;
            row.enabled = input.enabled;
            row.updated_at = now;
            row.clone()
        }
        None => {
            let id = t.next_id;
            t.next_id += 1;
            let provider = Provider {
                id,
                name: input.name,
                channel: input.channel,
                label: input.label,
                settings_json: input.settings_json,
                credential_strategy: input.credential_strategy,
                enabled: input.enabled,
                created_at: now,
                updated_at: now,
            };
            t.rows.push(provider.clone());
            provider
        }
    };

    table::store(&file, &t).await?;
    Ok(stored)
}

pub(super) async fn delete(root: &Path, id: i64) -> anyhow::Result<bool> {
    let file = path(root);
    let mut t = table::load::<Provider>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|p| p.id != id);
    let removed = t.rows.len() != before;
    if removed {
        table::store(&file, &t).await?;
    }
    Ok(removed)
}
