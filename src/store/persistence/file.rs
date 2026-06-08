//! File-system persistence backend.

use std::path::PathBuf;

use tokio::sync::Mutex;

mod impl_backend;
mod table;

mod identity;
mod provider;
mod routing;
mod rules;
mod settings;
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
        Ok(Self {
            root: data_dir,
            write: Mutex::new(()),
        })
    }
}

#[cfg(test)]
mod tests;
