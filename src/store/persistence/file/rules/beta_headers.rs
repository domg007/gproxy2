//! File-backend beta-header ops over `beta_headers.json`.

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{BetaHeader, BetaHeaderInput};

use crate::store::persistence::file::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("beta_headers.json")
}

pub(crate) async fn list(root: &Path, provider_id: i64) -> anyhow::Result<Vec<BetaHeader>> {
    Ok(table::load::<BetaHeader>(&path(root))
        .await?
        .rows
        .into_iter()
        .filter(|b| b.provider_id == provider_id)
        .collect())
}

pub(crate) async fn get(root: &Path, id: i64) -> anyhow::Result<Option<BetaHeader>> {
    Ok(table::load::<BetaHeader>(&path(root))
        .await?
        .rows
        .into_iter()
        .find(|b| b.id == id))
}

pub(crate) async fn upsert(root: &Path, input: BetaHeaderInput) -> anyhow::Result<BetaHeader> {
    let file = path(root);
    let mut t = table::load::<BetaHeader>(&file).await?;
    let now = now_secs();

    let stored = match input.id {
        Some(id) => {
            let row = t
                .rows
                .iter_mut()
                .find(|b| b.id == id)
                .ok_or_else(|| anyhow::anyhow!("beta header not found: {id}"))?;
            row.provider_id = input.provider_id;
            row.token = input.token;
            row.sort_order = input.sort_order;
            row.enabled = input.enabled;
            row.updated_at = now;
            row.clone()
        }
        None => {
            let id = t.next_id;
            t.next_id += 1;
            let header = BetaHeader {
                id,
                provider_id: input.provider_id,
                token: input.token,
                sort_order: input.sort_order,
                enabled: input.enabled,
                created_at: now,
                updated_at: now,
            };
            t.rows.push(header.clone());
            header
        }
    };

    table::store(&file, &t).await?;
    Ok(stored)
}

pub(crate) async fn delete(root: &Path, id: i64) -> anyhow::Result<bool> {
    let file = path(root);
    let mut t = table::load::<BetaHeader>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|b| b.id != id);
    let removed = t.rows.len() != before;
    if removed {
        table::store(&file, &t).await?;
    }
    Ok(removed)
}

pub(crate) async fn delete_by_provider(root: &Path, provider_id: i64) -> anyhow::Result<()> {
    let file = path(root);
    let mut t = table::load::<BetaHeader>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|b| b.provider_id != provider_id);
    if t.rows.len() != before {
        table::store(&file, &t).await?;
    }
    Ok(())
}
