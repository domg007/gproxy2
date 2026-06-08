//! Schema creation on connect. Derives `CREATE TABLE IF NOT EXISTS` from the
//! SeaORM entities for whatever dialect the connection uses (single source of
//! truth = the entity definitions; no separate migration crate yet).

use sea_orm::{ConnectionTrait, DatabaseConnection, Schema};

use super::entities::provider;

pub(super) async fn create_all(conn: &DatabaseConnection) -> anyhow::Result<()> {
    let backend = conn.get_database_backend();
    let schema = Schema::new(backend);

    let mut stmt = schema.create_table_from_entity(provider::Entity);
    stmt.if_not_exists();
    conn.execute(&stmt).await?;

    Ok(())
}
