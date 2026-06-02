//! Durable storage abstraction.
//!
//! Native impls: `file` (local disk, single-instance) and `db` (SeaORM, multi-instance).
//! Edge (wasm32) impl: `libsql` (libSQL/Turso over Hrana HTTP).
//! Domain code calls only trait methods.

#[cfg(not(target_arch = "wasm32"))]
pub mod db;
#[cfg(not(target_arch = "wasm32"))]
pub mod file;

#[cfg(target_arch = "wasm32")]
pub mod libsql;

#[cfg(not(target_arch = "wasm32"))]
pub use db::DbPersistence;
#[cfg(not(target_arch = "wasm32"))]
pub use file::FilePersistence;

#[cfg(target_arch = "wasm32")]
pub use libsql::LibsqlPersistence;

/// Durable storage abstraction.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait PersistenceBackend: Send + Sync {
    /// Backend kind label for diagnostics: "file" | "db" | "libsql".
    fn kind(&self) -> &'static str;

    /// Verify the backend is reachable/usable.
    async fn health(&self) -> anyhow::Result<()>;
}
