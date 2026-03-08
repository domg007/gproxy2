mod crypto;
pub mod entities;
mod migration;
mod optimize;
mod store_mutation;
mod store_query;
mod write_sink;

use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};

pub(crate) use crypto::DatabaseCipher;

#[derive(Clone)]
pub struct SeaOrmStorage {
    db: DatabaseConnection,
    cipher: Option<DatabaseCipher>,
}

impl SeaOrmStorage {
    pub async fn connect(dsn: &str, database_secret_key: Option<&str>) -> Result<Self, DbErr> {
        let mut options = ConnectOptions::new(dsn.to_string());
        optimize::configure_connect_options(&mut options);
        let db = Database::connect(options).await?;
        optimize::apply_after_connect(&db).await?;
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
        schema.sync(&self.db).await?;
        migration::run(&self.db).await?;
        optimize::apply_after_sync(&self.db).await
    }
}
