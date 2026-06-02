//! File-system persistence backend.

use std::path::PathBuf;

use async_trait::async_trait;

use super::PersistenceBackend;

/// Persistence backend backed by the local file system.
///
/// Suitable for single-instance deployments. The `root` directory is
/// created on [`open`](FilePersistence::open) if it does not exist.
pub struct FilePersistence {
    root: PathBuf,
}

impl FilePersistence {
    /// Open (and create if absent) the data directory at `data_dir`.
    pub async fn open(data_dir: PathBuf) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(&data_dir).await.map_err(|e| {
            anyhow::anyhow!("failed to create data dir {}: {e}", data_dir.display())
        })?;
        Ok(Self { root: data_dir })
    }
}

#[async_trait]
impl PersistenceBackend for FilePersistence {
    fn kind(&self) -> &'static str {
        "file"
    }

    async fn health(&self) -> anyhow::Result<()> {
        let probe = self.root.join(".gproxy_health_probe");
        tokio::fs::write(&probe, b"ok")
            .await
            .map_err(|e| anyhow::anyhow!("data dir is not writable: {e}"))?;
        tokio::fs::remove_file(&probe).await.ok();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_and_health_ok() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fp = FilePersistence::open(dir.path().to_path_buf())
            .await
            .expect("open");
        fp.health().await.expect("health");
    }
}
