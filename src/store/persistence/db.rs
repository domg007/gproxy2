//! Database persistence backend (SeaORM).

use async_trait::async_trait;
use sea_orm::{ConnectOptions, Database, DatabaseConnection};

use super::PersistenceBackend;

/// Persistence backend backed by a SeaORM-managed database.
///
/// Supports SQLite, PostgreSQL, and MySQL. Connect with
/// [`connect`](DbPersistence::connect), passing a standard connection DSN.
pub struct DbPersistence {
    conn: DatabaseConnection,
}

impl DbPersistence {
    /// Connect to the database identified by `dsn`.
    pub async fn connect(dsn: &str) -> anyhow::Result<Self> {
        let opts = ConnectOptions::new(dsn.to_string());
        let conn = Database::connect(opts)
            .await
            .map_err(|e| anyhow::anyhow!("db connect failed ({dsn}): {e}"))?;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sqlite_memory_connect_and_health() {
        let db = DbPersistence::connect("sqlite::memory:")
            .await
            .expect("connect");
        db.health().await.expect("health");
    }
}
