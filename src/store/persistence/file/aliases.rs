//! File-backend alias ops over `aliases.json`.

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{Alias, AliasInput};

use super::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("aliases.json")
}

pub(super) async fn list(root: &Path) -> anyhow::Result<Vec<Alias>> {
    Ok(table::load::<Alias>(&path(root)).await?.rows)
}

pub(super) async fn get_by_name(root: &Path, alias: &str) -> anyhow::Result<Option<Alias>> {
    Ok(table::load::<Alias>(&path(root))
        .await?
        .rows
        .into_iter()
        .find(|a| a.alias == alias))
}

pub(super) async fn upsert(root: &Path, input: AliasInput) -> anyhow::Result<Alias> {
    let file = path(root);
    let mut t = table::load::<Alias>(&file).await?;
    let now = now_secs();

    if let Some(existing) = t.rows.iter().find(|a| a.alias == input.alias)
        && Some(existing.id) != input.id
    {
        anyhow::bail!("alias already exists: {}", input.alias);
    }

    let stored = match input.id {
        Some(id) => {
            let row = t
                .rows
                .iter_mut()
                .find(|a| a.id == id)
                .ok_or_else(|| anyhow::anyhow!("alias not found: {id}"))?;
            row.alias = input.alias;
            row.route_id = input.route_id;
            row.updated_at = now;
            row.clone()
        }
        None => {
            let id = t.next_id;
            t.next_id += 1;
            let alias = Alias {
                id,
                alias: input.alias,
                route_id: input.route_id,
                created_at: now,
                updated_at: now,
            };
            t.rows.push(alias.clone());
            alias
        }
    };

    table::store(&file, &t).await?;
    Ok(stored)
}

pub(super) async fn delete(root: &Path, id: i64) -> anyhow::Result<bool> {
    let file = path(root);
    let mut t = table::load::<Alias>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|a| a.id != id);
    let removed = t.rows.len() != before;
    if removed {
        table::store(&file, &t).await?;
    }
    Ok(removed)
}

pub(super) async fn delete_by_route(root: &Path, route_id: i64) -> anyhow::Result<()> {
    let file = path(root);
    let mut t = table::load::<Alias>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|a| a.route_id != route_id);
    if t.rows.len() != before {
        table::store(&file, &t).await?;
    }
    Ok(())
}
