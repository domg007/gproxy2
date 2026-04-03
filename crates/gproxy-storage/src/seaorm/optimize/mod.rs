use sea_orm::{ConnectOptions, DatabaseConnection, DbErr};

pub(crate) fn configure_connect_options(_options: &mut ConnectOptions) {
    // TODO: database-specific pool and pragma settings
}

pub(crate) async fn apply_after_connect(_db: &DatabaseConnection) -> Result<(), DbErr> {
    Ok(())
}

pub(crate) async fn apply_after_sync(_db: &DatabaseConnection) -> Result<(), DbErr> {
    // TODO: create indexes
    Ok(())
}
