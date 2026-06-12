//! File-backend usage ops over `usages.json` (append-only).

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{Usage, UsageInput};

use crate::store::persistence::file::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("usages.json")
}

/// Append a usage row; `Ok(None)` when a row with the same `request_id` already
/// exists (idempotent settle, §17). The duplicate check is a linear scan —
/// O(rows) per append, acceptable for the dev-scale file backend.
pub(crate) async fn append(root: &Path, input: UsageInput) -> anyhow::Result<Option<Usage>> {
    let file = path(root);
    let mut t = table::load::<Usage>(&file).await?;
    if t.rows.iter().any(|r| r.request_id == input.request_id) {
        return Ok(None);
    }
    let now = now_secs();

    let id = t.next_id;
    t.next_id += 1;
    let usage = Usage {
        id,
        request_id: input.request_id,
        at: input.at,
        route_name: input.route_name,
        provider_id: input.provider_id,
        credential_id: input.credential_id,
        org_id: input.org_id,
        team_id: input.team_id,
        user_id: input.user_id,
        user_key_id: input.user_key_id,
        operation: input.operation,
        kind: input.kind,
        model: input.model,
        input_tokens: input.input_tokens,
        output_tokens: input.output_tokens,
        cache_read_tokens: input.cache_read_tokens,
        cache_creation_5m_tokens: input.cache_creation_5m_tokens,
        cache_creation_1h_tokens: input.cache_creation_1h_tokens,
        cost: input.cost,
        latency_ms: input.latency_ms,
        usage_source: input.usage_source,
        ended: input.ended,
        created_at: now,
        updated_at: now,
    };
    t.rows.push(usage.clone());

    table::store(&file, &t).await?;
    Ok(Some(usage))
}

pub(crate) async fn list(root: &Path, limit: u64) -> anyhow::Result<Vec<Usage>> {
    let mut rows = table::load::<Usage>(&path(root)).await?.rows;
    rows.sort_by_key(|r| std::cmp::Reverse(r.id));
    rows.truncate(limit as usize);
    Ok(rows)
}
