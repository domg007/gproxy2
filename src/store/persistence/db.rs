//! Database persistence backend (SeaORM).

use async_trait::async_trait;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};

use super::PersistenceBackend;
use super::records::{Provider, ProviderInput};

mod entities;
mod ops;
mod schema;

/// Persistence backend backed by a SeaORM-managed database.
///
/// Supports SQLite, PostgreSQL, and MySQL. Connect with
/// [`connect`](DbPersistence::connect), passing a standard connection DSN.
/// SeaORM is an internal detail of this backend; domain code uses only the
/// [`PersistenceBackend`] trait.
pub struct DbPersistence {
    conn: DatabaseConnection,
}

impl DbPersistence {
    /// Connect to the database identified by `dsn` and ensure the schema exists.
    pub async fn connect(dsn: &str) -> anyhow::Result<Self> {
        let opts = ConnectOptions::new(dsn.to_string());
        let conn = Database::connect(opts)
            .await
            .map_err(|e| anyhow::anyhow!("db connect failed: {e}"))?;
        schema::create_all(&conn).await?;
        Ok(Self { conn })
    }
}

#[async_trait]
impl PersistenceBackend for DbPersistence {
    fn kind(&self) -> &'static str {
        "db"
    }

    async fn health(&self) -> anyhow::Result<()> {
        self.conn
            .ping()
            .await
            .map_err(|e| anyhow::anyhow!("db ping failed: {e}"))
    }

    async fn list_providers(&self) -> anyhow::Result<Vec<Provider>> {
        ops::providers::list(&self.conn).await
    }

    async fn get_provider(&self, id: i64) -> anyhow::Result<Option<Provider>> {
        ops::providers::get(&self.conn, id).await
    }

    async fn get_provider_by_name(&self, name: &str) -> anyhow::Result<Option<Provider>> {
        ops::providers::get_by_name(&self.conn, name).await
    }

    async fn upsert_provider(&self, input: ProviderInput) -> anyhow::Result<Provider> {
        ops::providers::upsert(&self.conn, input).await
    }

    async fn delete_provider(&self, id: i64) -> anyhow::Result<bool> {
        ops::providers::delete(&self.conn, id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn mem() -> DbPersistence {
        DbPersistence::connect("sqlite::memory:")
            .await
            .expect("connect")
    }

    #[tokio::test]
    async fn sqlite_memory_connect_and_health() {
        mem().await.health().await.expect("health");
    }

    #[tokio::test]
    async fn provider_round_trip() {
        let db = mem().await;
        let created = db
            .upsert_provider(ProviderInput {
                id: None,
                name: "openai".to_owned(),
                channel: "openai".to_owned(),
                label: Some("OpenAI".to_owned()),
                settings_json: json!({"base_url": "https://api.openai.com"}),
                credential_strategy: "round_robin".to_owned(),
                enabled: true,
            })
            .await
            .expect("insert");
        assert!(created.id > 0);

        let fetched = db
            .get_provider_by_name("openai")
            .await
            .expect("get")
            .expect("some");
        assert_eq!(fetched, created);

        let updated = db
            .upsert_provider(ProviderInput {
                id: Some(created.id),
                name: "openai".to_owned(),
                channel: "openai".to_owned(),
                label: None,
                settings_json: json!({"base_url": "https://x"}),
                credential_strategy: "sticky".to_owned(),
                enabled: false,
            })
            .await
            .expect("update");
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.credential_strategy, "sticky");
        assert!(!updated.enabled);

        assert_eq!(db.list_providers().await.expect("list").len(), 1);
        assert!(db.delete_provider(created.id).await.expect("delete"));
        assert!(db.list_providers().await.expect("list").is_empty());
    }
}
