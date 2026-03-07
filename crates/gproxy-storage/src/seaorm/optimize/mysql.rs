use sea_orm::{ConnectOptions, ConnectionTrait, DbErr};

use super::{ManagedIndex, full_filter_indexes, mysql_index_exists};

pub(super) fn configure_connect_options(options: &mut ConnectOptions) {
    options.map_sqlx_mysql_opts(|options| options.statement_cache_capacity(512).charset("utf8mb4"));
}

pub(super) async fn apply_after_connect<C>(_db: &C) -> Result<(), DbErr>
where
    C: ConnectionTrait,
{
    Ok(())
}

pub(super) fn indexes() -> Vec<ManagedIndex> {
    full_filter_indexes()
}

pub(super) async fn index_exists<C>(
    db: &C,
    table_name: &str,
    index_name: &str,
) -> Result<bool, DbErr>
where
    C: ConnectionTrait,
{
    mysql_index_exists(db, table_name, index_name).await
}
