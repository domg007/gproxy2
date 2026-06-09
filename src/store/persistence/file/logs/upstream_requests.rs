//! File-backend upstream-request log ops over `upstream_requests.json`.

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{UpstreamRequest, UpstreamRequestInput};

use crate::store::persistence::file::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("upstream_requests.json")
}

pub(crate) async fn append(
    root: &Path,
    input: UpstreamRequestInput,
) -> anyhow::Result<UpstreamRequest> {
    let file = path(root);
    let mut t = table::load::<UpstreamRequest>(&file).await?;
    let now = now_secs();

    let id = t.next_id;
    t.next_id += 1;
    let req = UpstreamRequest {
        id,
        request_id: input.request_id,
        at: input.at,
        provider_id: input.provider_id,
        credential_id: input.credential_id,
        url: input.url,
        method: input.method,
        status: input.status,
        latency_ms: input.latency_ms,
        headers_json: input.headers_json,
        body: input.body,
        created_at: now,
        updated_at: now,
    };
    t.rows.push(req.clone());

    table::store(&file, &t).await?;
    Ok(req)
}

pub(crate) async fn list(root: &Path, request_id: &str) -> anyhow::Result<Vec<UpstreamRequest>> {
    Ok(table::load::<UpstreamRequest>(&path(root))
        .await?
        .rows
        .into_iter()
        .filter(|r| r.request_id == request_id)
        .collect())
}
