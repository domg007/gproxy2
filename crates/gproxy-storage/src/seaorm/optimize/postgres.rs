use sea_orm::{ConnectOptions, ConnectionTrait, DbErr};

use super::{ManagedIndex, partial_filter_indexes};

pub(super) fn configure_connect_options(options: &mut ConnectOptions) {
    options.map_sqlx_postgres_opts(|options| {
        options
            .application_name("gproxy")
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
