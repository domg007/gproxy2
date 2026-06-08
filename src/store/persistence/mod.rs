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

pub mod records;

#[cfg(not(target_arch = "wasm32"))]
pub use db::DbPersistence;
#[cfg(not(target_arch = "wasm32"))]
pub use file::FilePersistence;

#[cfg(target_arch = "wasm32")]
pub use libsql::LibsqlPersistence;

use records::{Provider, ProviderInput};

/// Durable storage abstraction.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait PersistenceBackend: Send + Sync {
    /// Backend kind label for diagnostics: "file" | "db" | "libsql".
    fn kind(&self) -> &'static str;

    /// Verify the backend is reachable/usable.
    async fn health(&self) -> anyhow::Result<()>;

    // ── providers (§8-B) ────────────────────────────────────────────────────

    /// List all providers.
    async fn list_providers(&self) -> anyhow::Result<Vec<Provider>>;

    /// Fetch a provider by id, or `None` if absent.
    async fn get_provider(&self, id: i64) -> anyhow::Result<Option<Provider>>;

    /// Fetch a provider by its unique name, or `None` if absent.
    async fn get_provider_by_name(&self, name: &str) -> anyhow::Result<Option<Provider>>;

    /// Insert (`input.id == None`) or update (`Some(id)`) a provider; returns
    /// the stored record with id and timestamps populated.
    async fn upsert_provider(&self, input: ProviderInput) -> anyhow::Result<Provider>;

    /// Delete a provider by id; returns whether a row was removed.
    async fn delete_provider(&self, id: i64) -> anyhow::Result<bool>;
}
