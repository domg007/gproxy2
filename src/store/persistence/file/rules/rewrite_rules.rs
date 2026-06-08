//! File-backend rewrite-rule ops over `rewrite_rules.json`.

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{RewriteRule, RewriteRuleInput};

use crate::store::persistence::file::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("rewrite_rules.json")
}

pub(crate) async fn list(root: &Path, provider_id: i64) -> anyhow::Result<Vec<RewriteRule>> {
    Ok(table::load::<RewriteRule>(&path(root))
        .await?
        .rows
        .into_iter()
        .filter(|r| r.provider_id == provider_id)
        .collect())
}

pub(crate) async fn get(root: &Path, id: i64) -> anyhow::Result<Option<RewriteRule>> {
    Ok(table::load::<RewriteRule>(&path(root))
        .await?
        .rows
        .into_iter()
        .find(|r| r.id == id))
}

pub(crate) async fn upsert(root: &Path, input: RewriteRuleInput) -> anyhow::Result<RewriteRule> {
    let file = path(root);
    let mut t = table::load::<RewriteRule>(&file).await?;
    let now = now_secs();

    let stored = match input.id {
        Some(id) => {
            let row = t
                .rows
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or_else(|| anyhow::anyhow!("rewrite rule not found: {id}"))?;
            row.provider_id = input.provider_id;
            row.path = input.path;
            row.action = input.action;
            row.value_json = input.value_json;
            row.filter_model_pattern = input.filter_model_pattern;
            row.filter_operation_keys = input.filter_operation_keys;
            row.sort_order = input.sort_order;
            row.enabled = input.enabled;
            row.updated_at = now;
            row.clone()
        }
        None => {
            let id = t.next_id;
            t.next_id += 1;
            let rule = RewriteRule {
                id,
                provider_id: input.provider_id,
                path: input.path,
                action: input.action,
                value_json: input.value_json,
                filter_model_pattern: input.filter_model_pattern,
                filter_operation_keys: input.filter_operation_keys,
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
    let mut t = table::load::<RewriteRule>(&file).await?;
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
    let mut t = table::load::<RewriteRule>(&file).await?;
    let before = t.rows.len();
    t.rows.retain(|r| r.provider_id != provider_id);
    if t.rows.len() != before {
        table::store(&file, &t).await?;
    }
    Ok(())
}
