//! Edge (wasm32) persistence backend backed by libSQL/Turso over Hrana HTTP.
//!
//! `LibsqlPersistence` wraps [`LibsqlClient`] and implements
//! [`PersistenceBackend`]. `health()` executes `SELECT 1`.
//!
//! Compile-checked on wasm32; round-trips require Turso credentials
//! (see ignored integration test).

use crate::store::libsql::LibsqlClient;

use super::PersistenceBackend;
use super::records::{
    Alias, AliasInput, Credential, CredentialInput, CredentialStatus, CredentialStatusInput,
    Provider, ProviderInput, ProviderModel, ProviderModelInput, Route, RouteInput, RouteMember,
    RouteMemberInput,
};

/// Edge persistence backend backed by a Turso/libSQL database via Hrana HTTP.
pub struct LibsqlPersistence {
    client: LibsqlClient,
}

impl LibsqlPersistence {
    /// Create a new persistence backend.
    ///
    /// `url` — Turso database URL (e.g. `https://<db>.turso.io`).
    /// `token` — Bearer auth token.
    pub fn connect(url: String, token: String) -> Self {
        Self {
            client: LibsqlClient::new(url, token),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl PersistenceBackend for LibsqlPersistence {
    fn kind(&self) -> &'static str {
        "libsql"
    }

    async fn health(&self) -> anyhow::Result<()> {
        self.client
            .execute("SELECT 1", &[])
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("libsql health failed: {e}"))
    }

    // TODO(edge): implement entity ops as hand-written SQLite SQL over the Hrana
    // client. Deferred to the edge AppState/entry phase (§10.8); the trait stays
    // coherent for the wasm build via these stubs.
    async fn list_providers(&self) -> anyhow::Result<Vec<Provider>> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn get_provider(&self, _id: i64) -> anyhow::Result<Option<Provider>> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn get_provider_by_name(&self, _name: &str) -> anyhow::Result<Option<Provider>> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn upsert_provider(&self, _input: ProviderInput) -> anyhow::Result<Provider> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn delete_provider(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn list_credentials(&self, _provider_id: i64) -> anyhow::Result<Vec<Credential>> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn get_credential(&self, _id: i64) -> anyhow::Result<Option<Credential>> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn upsert_credential(&self, _input: CredentialInput) -> anyhow::Result<Credential> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn delete_credential(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn list_credential_statuses(
        &self,
        _credential_id: i64,
    ) -> anyhow::Result<Vec<CredentialStatus>> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn upsert_credential_status(
        &self,
        _input: CredentialStatusInput,
    ) -> anyhow::Result<CredentialStatus> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn delete_credential_status(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql credential ops not implemented yet")
    }

    async fn list_routes(&self) -> anyhow::Result<Vec<Route>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn get_route(&self, _id: i64) -> anyhow::Result<Option<Route>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn get_route_by_name(&self, _name: &str) -> anyhow::Result<Option<Route>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn upsert_route(&self, _input: RouteInput) -> anyhow::Result<Route> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn delete_route(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn list_route_members(&self, _route_id: i64) -> anyhow::Result<Vec<RouteMember>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn upsert_route_member(&self, _input: RouteMemberInput) -> anyhow::Result<RouteMember> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn delete_route_member(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn list_aliases(&self) -> anyhow::Result<Vec<Alias>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn get_alias_by_name(&self, _alias: &str) -> anyhow::Result<Option<Alias>> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn upsert_alias(&self, _input: AliasInput) -> anyhow::Result<Alias> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn delete_alias(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql route ops not implemented yet")
    }

    async fn list_provider_models(&self, _provider_id: i64) -> anyhow::Result<Vec<ProviderModel>> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn upsert_provider_model(
        &self,
        _input: ProviderModelInput,
    ) -> anyhow::Result<ProviderModel> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }

    async fn delete_provider_model(&self, _id: i64) -> anyhow::Result<bool> {
        anyhow::bail!("libsql provider ops not implemented yet")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[wasm_bindgen_test::wasm_bindgen_test]
    #[ignore = "requires live Turso creds via GPROXY_TEST_TURSO_URL / GPROXY_TEST_TURSO_TOKEN"]
    async fn integration_health() {
        let url = std::env::var("GPROXY_TEST_TURSO_URL").expect("GPROXY_TEST_TURSO_URL");
        let token = std::env::var("GPROXY_TEST_TURSO_TOKEN").expect("GPROXY_TEST_TURSO_TOKEN");
        let backend = LibsqlPersistence::connect(url, token);
        backend.health().await.expect("health");
    }
}
