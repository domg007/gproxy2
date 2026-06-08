//! Durable storage abstraction.
//!
//! Native impls: `file` (local disk, single-instance) and `db` (SeaORM, multi-instance).
//! Edge (wasm32) impl: `libsql` (libSQL/Turso over Hrana HTTP).
//! Domain code calls only trait methods.

#[cfg(all(not(target_arch = "wasm32"), feature = "persist-db"))]
pub mod db;
#[cfg(all(not(target_arch = "wasm32"), feature = "persist-file"))]
pub mod file;

#[cfg(all(target_arch = "wasm32", feature = "persist-libsql"))]
pub mod libsql;

pub mod records;

#[cfg(all(not(target_arch = "wasm32"), feature = "persist-db"))]
pub use db::DbPersistence;
#[cfg(all(not(target_arch = "wasm32"), feature = "persist-file"))]
pub use file::FilePersistence;

#[cfg(all(target_arch = "wasm32", feature = "persist-libsql"))]
pub use libsql::LibsqlPersistence;

use records::{
    Alias, AliasInput, Credential, CredentialInput, CredentialStatus, CredentialStatusInput,
    Provider, ProviderInput, ProviderModel, ProviderModelInput, Route, RouteInput, RouteMember,
    RouteMemberInput,
};

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
    /// Application-level cascade: also removes the provider's credentials
    /// (and their statuses) and provider models.
    async fn delete_provider(&self, id: i64) -> anyhow::Result<bool>;

    // ── credentials (§8-B) ──────────────────────────────────────────────────

    /// List credentials in a provider's pool.
    async fn list_credentials(&self, provider_id: i64) -> anyhow::Result<Vec<Credential>>;

    /// Fetch a credential by id.
    async fn get_credential(&self, id: i64) -> anyhow::Result<Option<Credential>>;

    /// Insert or update a credential.
    async fn upsert_credential(&self, input: CredentialInput) -> anyhow::Result<Credential>;

    /// Delete a credential; cascades to its status snapshots.
    async fn delete_credential(&self, id: i64) -> anyhow::Result<bool>;

    /// List a credential's health snapshots.
    async fn list_credential_statuses(
        &self,
        credential_id: i64,
    ) -> anyhow::Result<Vec<CredentialStatus>>;

    /// Insert or update a credential status (unique per `(credential_id, channel)`).
    async fn upsert_credential_status(
        &self,
        input: CredentialStatusInput,
    ) -> anyhow::Result<CredentialStatus>;

    /// Delete a credential status by id.
    async fn delete_credential_status(&self, id: i64) -> anyhow::Result<bool>;

    // ── routes / members / aliases (§8-A) ───────────────────────────────────

    /// List all routes.
    async fn list_routes(&self) -> anyhow::Result<Vec<Route>>;

    /// Fetch a route by id.
    async fn get_route(&self, id: i64) -> anyhow::Result<Option<Route>>;

    /// Fetch a route by its unique name.
    async fn get_route_by_name(&self, name: &str) -> anyhow::Result<Option<Route>>;

    /// Insert or update a route.
    async fn upsert_route(&self, input: RouteInput) -> anyhow::Result<Route>;

    /// Delete a route; cascades to its members and aliases.
    async fn delete_route(&self, id: i64) -> anyhow::Result<bool>;

    /// List a route's members.
    async fn list_route_members(&self, route_id: i64) -> anyhow::Result<Vec<RouteMember>>;

    /// Insert or update a route member.
    async fn upsert_route_member(&self, input: RouteMemberInput) -> anyhow::Result<RouteMember>;

    /// Delete a route member by id.
    async fn delete_route_member(&self, id: i64) -> anyhow::Result<bool>;

    /// List all aliases.
    async fn list_aliases(&self) -> anyhow::Result<Vec<Alias>>;

    /// Fetch an alias by its unique alias name.
    async fn get_alias_by_name(&self, alias: &str) -> anyhow::Result<Option<Alias>>;

    /// Insert or update an alias.
    async fn upsert_alias(&self, input: AliasInput) -> anyhow::Result<Alias>;

    /// Delete an alias by id.
    async fn delete_alias(&self, id: i64) -> anyhow::Result<bool>;

    // ── provider models (§8-A) ──────────────────────────────────────────────

    /// List a provider's models.
    async fn list_provider_models(&self, provider_id: i64) -> anyhow::Result<Vec<ProviderModel>>;

    /// Insert or update a provider model.
    async fn upsert_provider_model(
        &self,
        input: ProviderModelInput,
    ) -> anyhow::Result<ProviderModel>;

    /// Delete a provider model by id.
    async fn delete_provider_model(&self, id: i64) -> anyhow::Result<bool>;
}
