//! File-backend system-prelude ops over `preludes_system.json`.

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{PreludeSystem, PreludeSystemInput};

use crate::store::persistence::file::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("preludes_system.json")
}

pub(crate) async fn list(root: &Path, provider_id: i64) -> anyhow::Result<Vec<PreludeSystem>> {
    Ok(table::load::<PreludeSystem>(&path(root))
        .await?
        .rows
        .into_iter()
        .filter(|p| p.provider_id == provider_id)
        .collect())
}

pub(crate) async fn get(root: &Path, id: i64) -> anyhow::Result<Option<PreludeSystem>> {
    Ok(table::load::<PreludeSystem>(&path(root))
        .await?
        .rows
        .into_iter()
        .find(|p| p.id == id))
}

pub(crate) async fn upsert(
    root: &Path,
    input: PreludeSystemInput,
) -> anyhow::Result<PreludeSystem> {
    let file = path(root);
    let mut t = table::load::<PreludeSystem>(&file).await?;
    let now = now_secs();

    let stored = match input.id {
        Some(id) => {
            let row = t
                .rows
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or_else(|| anyhow::anyhow!("system prelude not found: {id}"))?;
            row.provider_id = input.provider_id;
            row.text = input.text;
            row.sort_order = input.sort_order;
            row.enabled = input.enabled;
            row.updated_at = now;
            row.clone()
        }
        None => {
            let id = t.next_id;
            t.next_id += 1;
            let prelude = PreludeSystem {
                id,
                provider_id: input.provider_id,
                text: input.text,
                sort_order: input.sort_order,
                enabled: input.enabled,
                created_at: now,
                updated_at: now,
            };
            t.rows.push(prelude.clone());
            prelude
        }
    };

    table::store(&file, &t).await?;
    Ok(stored)
}

pub(crate) async fn delete(root: &Path, id: i64) -> anyhow::Result<bool> {
    let file = path(root);
    let mut t = table::load::<PreludeSystem>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|p| p.id != id);
    let removed = t.rows.len() != before;
    if removed {
        table::store(&file, &t).await?;
    }
    Ok(removed)
}

pub(crate) async fn delete_by_provider(root: &Path, provider_id: i64) -> anyhow::Result<()> {
    let file = path(root);
    let mut t = table::load::<PreludeSystem>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|p| p.provider_id != provider_id);
    if t.rows.len() != before {
        table::store(&file, &t).await?;
    }
    Ok(())
}
