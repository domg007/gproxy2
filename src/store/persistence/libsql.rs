//! Edge (wasm32) persistence backend backed by libSQL/Turso over Hrana HTTP.
//!
//! `LibsqlPersistence` wraps [`LibsqlClient`] and implements
//! [`PersistenceBackend`]. `health()` executes `SELECT 1`.
//!
//! Compile-checked on wasm32; round-trips require Turso credentials
//! (see ignored integration test).

use crate::store::libsql::LibsqlClient;

use super::PersistenceBackend;
use super::records::{Provider, ProviderInput};

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
