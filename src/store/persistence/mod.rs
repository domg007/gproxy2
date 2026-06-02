//! Durable storage abstraction. Two deployment-selected impls: `file`
//! (local disk, single-instance) and `db` (SeaORM, multi-instance).
//! Domain code calls only trait methods, never SeaORM directly. The
//! method surface grows with the data model; for the skeleton it is just
//! identity + a health check.

use async_trait::async_trait;

pub mod db;
pub mod file;

pub use db::DbPersistence;
pub use file::FilePersistence;

/// Durable storage abstraction.
#[async_trait]
pub trait PersistenceBackend: Send + Sync {
    /// Backend kind label for diagnostics: "file" | "db".
    fn kind(&self) -> &'static str;

    /// Verify the backend is reachable/usable.
    async fn health(&self) -> anyhow::Result<()>;
}
