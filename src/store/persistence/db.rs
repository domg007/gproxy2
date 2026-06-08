//! Database persistence backend (SeaORM).

use sea_orm::{ConnectOptions, Database, DatabaseConnection};

mod entities;
mod impl_backend;
mod ops;
mod schema;

/// Persistence backend backed by a SeaORM-managed database.
///
/// Supports SQLite, PostgreSQL, and MySQL. Connect with
/// [`connect`](DbPersistence::connect), passing a standard connection DSN.
/// SeaORM is an internal detail of this backend; domain code uses only the
/// [`PersistenceBackend`](super::PersistenceBackend) trait.
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

#[cfg(test)]
mod tests;
