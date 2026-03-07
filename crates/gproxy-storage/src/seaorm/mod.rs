mod crypto;
pub mod entities;
mod store_mutation;
mod store_query;
mod write_sink;

use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, DbErr};

pub(crate) use crypto::DatabaseCipher;

#[derive(Clone)]
pub struct SeaOrmStorage {
    db: DatabaseConnection,
    cipher: Option<DatabaseCipher>,
}

impl SeaOrmStorage {
    pub async fn connect(dsn: &str, database_secret_key: Option<&str>) -> Result<Self, DbErr> {
        let db = Database::connect(dsn).await?;
        if db.get_database_backend() == DatabaseBackend::Sqlite {
            db.execute_unprepared("PRAGMA foreign_keys = ON").await?;
        }
        let cipher = DatabaseCipher::from_optional_secret(database_secret_key)
            .map_err(|err| DbErr::Custom(format!("load DATABASE_SECRET_KEY: {err}")))?;
        Ok(Self { db, cipher })
    }

    pub fn connection(&self) -> &DatabaseConnection {
        &self.db
    }

    pub(crate) fn cipher(&self) -> Option<&DatabaseCipher> {
        self.cipher.as_ref()
    }

    /// SeaORM 2.0 entity-first schema sync.
    pub async fn sync(&self) -> Result<(), DbErr> {
        let schema = self
            .db
            .get_schema_registry("gproxy_storage::seaorm::entities::*");
        schema.sync(&self.db).await
    }
}
