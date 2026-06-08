//! File-backend cache-breakpoint ops over `cache_breakpoints.json`.

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{CacheBreakpoint, CacheBreakpointInput};

use crate::store::persistence::file::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("cache_breakpoints.json")
}

pub(crate) async fn list(root: &Path, provider_id: i64) -> anyhow::Result<Vec<CacheBreakpoint>> {
    Ok(table::load::<CacheBreakpoint>(&path(root))
        .await?
        .rows
        .into_iter()
        .filter(|c| c.provider_id == provider_id)
        .collect())
}

pub(crate) async fn get(root: &Path, id: i64) -> anyhow::Result<Option<CacheBreakpoint>> {
    Ok(table::load::<CacheBreakpoint>(&path(root))
        .await?
        .rows
        .into_iter()
        .find(|c| c.id == id))
}

pub(crate) async fn upsert(
    root: &Path,
    input: CacheBreakpointInput,
) -> anyhow::Result<CacheBreakpoint> {
    let file = path(root);
    let mut t = table::load::<CacheBreakpoint>(&file).await?;
    let now = now_secs();

    let stored = match input.id {
        Some(id) => {
            let row = t
                .rows
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| anyhow::anyhow!("cache breakpoint not found: {id}"))?;
            row.provider_id = input.provider_id;
            row.target = input.target;
            row.position = input.position;
            row.index = input.index;
            row.ttl = input.ttl;
            row.sort_order = input.sort_order;
            row.enabled = input.enabled;
            row.updated_at = now;
            row.clone()
        }
        None => {
            let id = t.next_id;
            t.next_id += 1;
            let bp = CacheBreakpoint {
                id,
                provider_id: input.provider_id,
                target: input.target,
                position: input.position,
                index: input.index,
                ttl: input.ttl,
                sort_order: input.sort_order,
                enabled: input.enabled,
                created_at: now,
                updated_at: now,
            };
            t.rows.push(bp.clone());
            bp
        }
    };

    table::store(&file, &t).await?;
    Ok(stored)
}

pub(crate) async fn delete(root: &Path, id: i64) -> anyhow::Result<bool> {
    let file = path(root);
    let mut t = table::load::<CacheBreakpoint>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|c| c.id != id);
    let removed = t.rows.len() != before;
    if removed {
        table::store(&file, &t).await?;
    }
    Ok(removed)
}

pub(crate) async fn delete_by_provider(root: &Path, provider_id: i64) -> anyhow::Result<()> {
    let file = path(root);
    let mut t = table::load::<CacheBreakpoint>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|c| c.provider_id != provider_id);
    if t.rows.len() != before {
        table::store(&file, &t).await?;
    }
    Ok(())
}
