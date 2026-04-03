use sea_orm::{ConnectOptions, DatabaseBackend, DatabaseConnection, DbErr, Statement};
use sea_orm::ConnectionTrait;

pub(crate) fn configure_connect_options(options: &mut ConnectOptions) {
    let url = options.get_url().to_string();
    if url.starts_with("sqlite:") {
        options.map_sqlx_sqlite_opts(|opts| {
            use sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
            opts.foreign_keys(true)
                .busy_timeout(std::time::Duration::from_secs(5))
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Normal)
                .statement_cache_capacity(512)
        });
    } else if url.starts_with("mysql:") {
        options.map_sqlx_mysql_opts(|opts| {
            opts.statement_cache_capacity(512)
                .charset("utf8mb4")
        });
    } else if url.starts_with("postgres:") {
        options.map_sqlx_postgres_opts(|opts| {
            opts.application_name("gproxy")
                .statement_cache_capacity(512)
        });
    }
}

pub(crate) async fn apply_after_connect(_db: &DatabaseConnection) -> Result<(), DbErr> {
    Ok(())
}

pub(crate) async fn apply_after_sync(db: &DatabaseConnection) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    for (table, name, sql) in common_indexes(backend) {
        match backend {
            DatabaseBackend::MySql => {
                if !mysql_index_exists(db, table, name).await? {
                    db.execute_unprepared(&sql).await?;
                }
            }
            _ => {
                db.execute_unprepared(&sql).await?;
            }
        }
    }
    Ok(())
}

fn common_indexes(backend: DatabaseBackend) -> Vec<(&'static str, &'static str, String)> {
    let if_not_exists = match backend {
        DatabaseBackend::MySql => "",
        _ => "IF NOT EXISTS ",
    };
    vec![
        ("providers", "idx_providers_channel",
            format!("CREATE INDEX {if_not_exists}idx_providers_channel ON providers (channel)")),
        ("credentials", "idx_credentials_provider_id",
            format!("CREATE INDEX {if_not_exists}idx_credentials_provider_id ON credentials (provider_id)")),
        ("user_keys", "idx_user_keys_user_id",
            format!("CREATE INDEX {if_not_exists}idx_user_keys_user_id ON user_keys (user_id)")),
        ("usages", "idx_usages_at_trace",
            format!("CREATE INDEX {if_not_exists}idx_usages_at_trace ON usages (at DESC, trace_id DESC)")),
        ("upstream_requests", "idx_upstream_requests_at_trace",
            format!("CREATE INDEX {if_not_exists}idx_upstream_requests_at_trace ON upstream_requests (at DESC, trace_id DESC)")),
        ("downstream_requests", "idx_downstream_requests_at_trace",
            format!("CREATE INDEX {if_not_exists}idx_downstream_requests_at_trace ON downstream_requests (at DESC, trace_id DESC)")),
        ("models", "idx_models_provider_id",
            format!("CREATE INDEX {if_not_exists}idx_models_provider_id ON models (provider_id)")),
        ("user_model_permissions", "idx_user_model_permissions_user_id",
            format!("CREATE INDEX {if_not_exists}idx_user_model_permissions_user_id ON user_model_permissions (user_id)")),
        ("user_rate_limits", "idx_user_rate_limits_user_id",
            format!("CREATE INDEX {if_not_exists}idx_user_rate_limits_user_id ON user_rate_limits (user_id)")),
    ]
}

async fn mysql_index_exists(
    db: &DatabaseConnection,
    table_name: &str,
    index_name: &str,
) -> Result<bool, DbErr> {
    let sql = format!(
        "SELECT COUNT(*) as cnt FROM information_schema.STATISTICS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{}' AND INDEX_NAME = '{}'",
        table_name, index_name
    );
    let result = db
        .query_one_raw(Statement::from_string(DatabaseBackend::MySql, sql))
        .await?;
    match result {
        Some(row) => {
            let cnt: i64 = row.try_get("", "cnt").unwrap_or(0);
            Ok(cnt > 0)
        }
        None => Ok(false),
    }
}
