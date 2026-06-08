//! File-backend sanitize-rule ops over `sanitize_rules.json`.

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{SanitizeRule, SanitizeRuleInput};

use crate::store::persistence::file::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("sanitize_rules.json")
}

pub(crate) async fn list(root: &Path, provider_id: i64) -> anyhow::Result<Vec<SanitizeRule>> {
    Ok(table::load::<SanitizeRule>(&path(root))
        .await?
        .rows
        .into_iter()
        .filter(|r| r.provider_id == provider_id)
        .collect())
}

pub(crate) async fn get(root: &Path, id: i64) -> anyhow::Result<Option<SanitizeRule>> {
    Ok(table::load::<SanitizeRule>(&path(root))
        .await?
        .rows
        .into_iter()
        .find(|r| r.id == id))
}

pub(crate) async fn upsert(root: &Path, input: SanitizeRuleInput) -> anyhow::Result<SanitizeRule> {
    let file = path(root);
    let mut t = table::load::<SanitizeRule>(&file).await?;
    let now = now_secs();

    let stored = match input.id {
        Some(id) => {
            let row = t
                .rows
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or_else(|| anyhow::anyhow!("sanitize rule not found: {id}"))?;
            row.provider_id = input.provider_id;
            row.pattern = input.pattern;
            row.replacement = input.replacement;
            row.sort_order = input.sort_order;
            row.enabled = input.enabled;
            row.updated_at = now;
            row.clone()
        }
        None => {
            let id = t.next_id;
            t.next_id += 1;
            let rule = SanitizeRule {
                id,
                provider_id: input.provider_id,
                pattern: input.pattern,
                replacement: input.replacement,
                sort_order: input.sort_order,
                enabled: input.enabled,
                created_at: now,
                updated_at: now,
            };
            t.rows.push(rule.clone());
            rule
        }
    };

    table::store(&file, &t).await?;
    Ok(stored)
}

pub(crate) async fn delete(root: &Path, id: i64) -> anyhow::Result<bool> {
    let file = path(root);
    let mut t = table::load::<SanitizeRule>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|r| r.id != id);
    let removed = t.rows.len() != before;
    if removed {
        table::store(&file, &t).await?;
    }
    Ok(removed)
}

pub(crate) async fn delete_by_provider(root: &Path, provider_id: i64) -> anyhow::Result<()> {
    let file = path(root);
    let mut t = table::load::<SanitizeRule>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|r| r.provider_id != provider_id);
    if t.rows.len() != before {
        table::store(&file, &t).await?;
    }
    Ok(())
}
