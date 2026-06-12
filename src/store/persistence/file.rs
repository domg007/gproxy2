//! File-system persistence backend.

use std::path::PathBuf;

use tokio::sync::Mutex;

mod impl_backend;
mod table;

mod authz;
mod identity;
mod logs;
mod metrics;
mod provider;
mod routing;
mod settings;
mod tokenizers;
mod transform;
mod usage;

/// Persistence backend backed by the local file system.
///
/// Suitable for single-instance deployments. The `root` directory is
/// created on [`open`](FilePersistence::open) if it does not exist. Each table
/// is one JSON file under `root`; the `write` mutex serializes mutations
/// (control-plane data is low-write, so whole-file read/rewrite is fine).
pub struct FilePersistence {
    root: PathBuf,
    write: Mutex<()>,
}

impl FilePersistence {
    /// Open (and create if absent) the data directory at `data_dir`.
    ///
    /// Only ensures the directory exists; write-permission is verified by
    /// [`health`](FilePersistence::health), which callers should invoke at startup.
    pub async fn open(data_dir: PathBuf) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(&data_dir).await.map_err(|e| {
            anyhow::anyhow!("failed to create data dir {}: {e}", data_dir.display())
        })?;
        stamp_schema_version(&data_dir).await?;
        Ok(Self {
            root: data_dir,
            write: Mutex::new(()),
        })
    }
}

/// The file backend is schemaless JSON, so there are no table migrations — but
/// for symmetry with the SQL backends we record a version stamp. Written once;
/// left untouched if it already exists (an existing store is already at this
/// version's on-disk shape).
async fn stamp_schema_version(root: &std::path::Path) -> anyhow::Result<()> {
    use crate::store::persistence::migrations::BASELINE_VERSION;

    let path = root.join("schema_version.json");
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Ok(());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let body = serde_json::json!({ "version": BASELINE_VERSION, "applied_at": now });
    tokio::fs::write(&path, serde_json::to_vec_pretty(&body)?)
        .await
        .map_err(|e| anyhow::anyhow!("failed to write schema_version.json: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests;
