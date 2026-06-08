//! File-backend downstream-request log ops over `downstream_requests.json`.

use std::path::{Path, PathBuf};

use crate::store::persistence::records::{DownstreamRequest, DownstreamRequestInput};

use crate::store::persistence::file::table::{self, now_secs};

fn path(root: &Path) -> PathBuf {
    root.join("downstream_requests.json")
}

pub(crate) async fn append(
    root: &Path,
    input: DownstreamRequestInput,
) -> anyhow::Result<DownstreamRequest> {
    let file = path(root);
    let mut t = table::load::<DownstreamRequest>(&file).await?;
    let now = now_secs();

    let id = t.next_id;
    t.next_id += 1;
    let req = DownstreamRequest {
        id,
        request_id: input.request_id,
        at: input.at,
        method: input.method,
        path: input.path,
        query: input.query,
        status: input.status,
        headers_json: input.headers_json,
        body: input.body,
        created_at: now,
        updated_at: now,
    };
    t.rows.push(req.clone());

    table::store(&file, &t).await?;
    Ok(req)
}

pub(crate) async fn list(root: &Path, request_id: &str) -> anyhow::Result<Vec<DownstreamRequest>> {
    Ok(table::load::<DownstreamRequest>(&path(root))
        .await?
        .rows
        .into_iter()
        .filter(|r| r.request_id == request_id)
        .collect())
}
