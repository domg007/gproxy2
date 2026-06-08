//! File-backend user-key ops over `user_keys.json`. `api_key_digest` is unique.

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{UserKey, UserKeyInput};

use crate::store::persistence::file::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("user_keys.json")
}

pub(crate) async fn list(root: &Path, user_id: i64) -> anyhow::Result<Vec<UserKey>> {
    Ok(table::load::<UserKey>(&path(root))
        .await?
        .rows
        .into_iter()
        .filter(|k| k.user_id == user_id)
        .collect())
}

pub(crate) async fn get(root: &Path, id: i64) -> anyhow::Result<Option<UserKey>> {
    Ok(table::load::<UserKey>(&path(root))
        .await?
        .rows
        .into_iter()
        .find(|k| k.id == id))
}

pub(crate) async fn find_by_digest(root: &Path, digest: &str) -> anyhow::Result<Option<UserKey>> {
    Ok(table::load::<UserKey>(&path(root))
        .await?
        .rows
        .into_iter()
        .find(|k| k.api_key_digest == digest))
}

pub(crate) async fn upsert(root: &Path, input: UserKeyInput) -> anyhow::Result<UserKey> {
    let file = path(root);
    let mut t = table::load::<UserKey>(&file).await?;
    let now = now_secs();

    if let Some(existing) = t
        .rows
        .iter()
        .find(|k| k.api_key_digest == input.api_key_digest)
        && Some(existing.id) != input.id
    {
        anyhow::bail!("user key digest already exists: {}", input.api_key_digest);
    }

    let stored = match input.id {
        Some(id) => {
            let row = t
                .rows
                .iter_mut()
                .find(|k| k.id == id)
                .ok_or_else(|| anyhow::anyhow!("user key not found: {id}"))?;
            row.user_id = input.user_id;
            row.api_key_ciphertext = input.api_key_ciphertext;
            row.api_key_digest = input.api_key_digest;
            row.label = input.label;
            row.enabled = input.enabled;
            row.updated_at = now;
            row.clone()
        }
        None => {
            let id = t.next_id;
            t.next_id += 1;
            let key = UserKey {
                id,
                user_id: input.user_id,
                api_key_ciphertext: input.api_key_ciphertext,
                api_key_digest: input.api_key_digest,
                label: input.label,
                enabled: input.enabled,
                created_at: now,
                updated_at: now,
            };
            t.rows.push(key.clone());
            key
        }
    };

    table::store(&file, &t).await?;
    Ok(stored)
}

pub(crate) async fn delete(root: &Path, id: i64) -> anyhow::Result<bool> {
    let file = path(root);
    let mut t = table::load::<UserKey>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|k| k.id != id);
    let removed = t.rows.len() != before;
    if removed {
        table::store(&file, &t).await?;
    }
    Ok(removed)
}

pub(crate) async fn delete_by_user(root: &Path, user_id: i64) -> anyhow::Result<()> {
    let file = path(root);
    let mut t = table::load::<UserKey>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|k| k.user_id != user_id);
    if t.rows.len() != before {
        table::store(&file, &t).await?;
    }
    Ok(())
}
