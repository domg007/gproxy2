use std::time::Duration;

use sea_orm::{ConnectOptions, ConnectionTrait, DbErr};
use sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};

use super::{ManagedIndex, partial_filter_indexes};

pub(super) fn configure_connect_options(options: &mut ConnectOptions) {
    options.map_sqlx_sqlite_opts(|options| {
        options
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .statement_cache_capacity(512)
    });
}

pub(super) async fn apply_after_connect<C>(_db: &C) -> Result<(), DbErr>
where
    C: ConnectionTrait,
{
    Ok(())
}

pub(super) fn indexes() -> Vec<ManagedIndex> {
    partial_filter_indexes()
}
