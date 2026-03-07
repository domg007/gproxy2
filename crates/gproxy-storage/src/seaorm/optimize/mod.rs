use sea_orm::sea_query::{Alias, Expr, Index, IndexCreateStatement, IndexOrder, Query};
use sea_orm::{
    ConditionalStatement, ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, ExprTrait,
};

use super::entities::{
    credentials, downstream_requests, providers, upstream_requests, usages, user_keys,
};
use sea_orm::ConnectOptions;

mod mysql;
mod postgres;
mod sqlite;

#[derive(Clone)]
pub(crate) struct ManagedIndex {
    pub(crate) table_name: &'static str,
    pub(crate) index_name: &'static str,
    pub(crate) statement: IndexCreateStatement,
}

impl ManagedIndex {
    fn new(
        table_name: &'static str,
        index_name: &'static str,
        statement: IndexCreateStatement,
    ) -> Self {
        Self {
            table_name,
            index_name,
            statement,
        }
    }
}

pub(crate) fn configure_connect_options(options: &mut ConnectOptions) {
    if DatabaseBackend::Sqlite.is_prefix_of(options.get_url()) {
        sqlite::configure_connect_options(options);
    } else if DatabaseBackend::MySql.is_prefix_of(options.get_url()) {
        mysql::configure_connect_options(options);
    } else if DatabaseBackend::Postgres.is_prefix_of(options.get_url()) {
        postgres::configure_connect_options(options);
    }
}

pub(crate) async fn apply_after_connect(db: &DatabaseConnection) -> Result<(), DbErr> {
    match db.get_database_backend() {
        DatabaseBackend::Sqlite => sqlite::apply_after_connect(db).await,
        DatabaseBackend::MySql => mysql::apply_after_connect(db).await,
        DatabaseBackend::Postgres => postgres::apply_after_connect(db).await,
        _ => Ok(()),
    }
}

pub(crate) async fn apply_after_sync(db: &DatabaseConnection) -> Result<(), DbErr> {
    apply_indexes(db, common_indexes()).await?;
    match db.get_database_backend() {
        DatabaseBackend::Sqlite => apply_indexes(db, sqlite::indexes()).await,
        DatabaseBackend::MySql => apply_indexes(db, mysql::indexes()).await,
        DatabaseBackend::Postgres => apply_indexes(db, postgres::indexes()).await,
        _ => Ok(()),
    }
}

async fn apply_indexes<C>(db: &C, indexes: Vec<ManagedIndex>) -> Result<(), DbErr>
where
    C: ConnectionTrait,
{
    for index in indexes {
        if db.get_database_backend() == DatabaseBackend::MySql
            && mysql::index_exists(db, index.table_name, index.index_name).await?
        {
            continue;
        }
        db.execute(&index.statement).await?;
    }
    Ok(())
}

fn common_indexes() -> Vec<ManagedIndex> {
    vec![
        providers_channel_index(),
        credentials_provider_index(),
        user_keys_user_index(),
        usages_at_trace_index(),
        upstream_at_trace_index(),
        downstream_at_trace_index(),
    ]
}

pub(super) fn full_filter_indexes() -> Vec<ManagedIndex> {
    vec![
        usages_user_at_trace_index(false),
        usages_user_key_at_trace_index(false),
        usages_provider_at_trace_index(false),
        usages_model_at_trace_index(false),
        upstream_provider_at_trace_index(false),
        upstream_credential_at_trace_index(false),
        downstream_user_at_trace_index(false),
        downstream_user_key_at_trace_index(false),
    ]
}

pub(super) fn partial_filter_indexes() -> Vec<ManagedIndex> {
    vec![
        usages_user_at_trace_index(true),
        usages_user_key_at_trace_index(true),
        usages_provider_at_trace_index(true),
        usages_model_at_trace_index(true),
        upstream_provider_at_trace_index(true),
        upstream_credential_at_trace_index(true),
        downstream_user_at_trace_index(true),
        downstream_user_key_at_trace_index(true),
    ]
}

fn providers_channel_index() -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-providers-channel")
        .table(providers::Entity)
        .col(providers::Column::Channel)
        .if_not_exists();
    ManagedIndex::new("providers", "idx-providers-channel", stmt.take())
}

fn credentials_provider_index() -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-credentials-provider-id")
        .table(credentials::Entity)
        .col(credentials::Column::ProviderId)
        .if_not_exists();
    ManagedIndex::new("credentials", "idx-credentials-provider-id", stmt.take())
}

fn user_keys_user_index() -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-user-keys-user-id")
        .table(user_keys::Entity)
        .col(user_keys::Column::UserId)
        .if_not_exists();
    ManagedIndex::new("user_keys", "idx-user-keys-user-id", stmt.take())
}

fn usages_at_trace_index() -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-usages-at-trace")
        .table(usages::Entity)
        .col((usages::Column::At, IndexOrder::Desc))
        .col((usages::Column::TraceId, IndexOrder::Desc))
        .if_not_exists();
    ManagedIndex::new("usages", "idx-usages-at-trace", stmt.take())
}

fn upstream_at_trace_index() -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-upstream-requests-at-trace")
        .table(upstream_requests::Entity)
        .col((upstream_requests::Column::At, IndexOrder::Desc))
        .col((upstream_requests::Column::TraceId, IndexOrder::Desc))
        .if_not_exists();
    ManagedIndex::new(
        "upstream_requests",
        "idx-upstream-requests-at-trace",
        stmt.take(),
    )
}

fn downstream_at_trace_index() -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-downstream-requests-at-trace")
        .table(downstream_requests::Entity)
        .col((downstream_requests::Column::At, IndexOrder::Desc))
        .col((downstream_requests::Column::TraceId, IndexOrder::Desc))
        .if_not_exists();
    ManagedIndex::new(
        "downstream_requests",
        "idx-downstream-requests-at-trace",
        stmt.take(),
    )
}

fn usages_user_at_trace_index(partial: bool) -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-usages-user-at-trace")
        .table(usages::Entity)
        .col(usages::Column::UserId)
        .col((usages::Column::At, IndexOrder::Desc))
        .col((usages::Column::TraceId, IndexOrder::Desc))
        .if_not_exists();
    if partial {
        stmt.and_where(Expr::col(usages::Column::UserId).is_not_null());
    }
    ManagedIndex::new("usages", "idx-usages-user-at-trace", stmt.take())
}

fn usages_user_key_at_trace_index(partial: bool) -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-usages-user-key-at-trace")
        .table(usages::Entity)
        .col(usages::Column::UserKeyId)
        .col((usages::Column::At, IndexOrder::Desc))
        .col((usages::Column::TraceId, IndexOrder::Desc))
        .if_not_exists();
    if partial {
        stmt.and_where(Expr::col(usages::Column::UserKeyId).is_not_null());
    }
    ManagedIndex::new("usages", "idx-usages-user-key-at-trace", stmt.take())
}

fn usages_provider_at_trace_index(partial: bool) -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-usages-provider-at-trace")
        .table(usages::Entity)
        .col(usages::Column::ProviderId)
        .col((usages::Column::At, IndexOrder::Desc))
        .col((usages::Column::TraceId, IndexOrder::Desc))
        .if_not_exists();
    if partial {
        stmt.and_where(Expr::col(usages::Column::ProviderId).is_not_null());
    }
    ManagedIndex::new("usages", "idx-usages-provider-at-trace", stmt.take())
}

fn usages_model_at_trace_index(partial: bool) -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-usages-model-at-trace")
        .table(usages::Entity)
        .col(usages::Column::Model)
        .col((usages::Column::At, IndexOrder::Desc))
        .col((usages::Column::TraceId, IndexOrder::Desc))
        .if_not_exists();
    if partial {
        stmt.and_where(Expr::col(usages::Column::Model).is_not_null());
    }
    ManagedIndex::new("usages", "idx-usages-model-at-trace", stmt.take())
}

fn upstream_provider_at_trace_index(partial: bool) -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-upstream-provider-at-trace")
        .table(upstream_requests::Entity)
        .col(upstream_requests::Column::ProviderId)
        .col((upstream_requests::Column::At, IndexOrder::Desc))
        .col((upstream_requests::Column::TraceId, IndexOrder::Desc))
        .if_not_exists();
    if partial {
        stmt.and_where(Expr::col(upstream_requests::Column::ProviderId).is_not_null());
    }
    ManagedIndex::new(
        "upstream_requests",
        "idx-upstream-provider-at-trace",
        stmt.take(),
    )
}

fn upstream_credential_at_trace_index(partial: bool) -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-upstream-credential-at-trace")
        .table(upstream_requests::Entity)
        .col(upstream_requests::Column::CredentialId)
        .col((upstream_requests::Column::At, IndexOrder::Desc))
        .col((upstream_requests::Column::TraceId, IndexOrder::Desc))
        .if_not_exists();
    if partial {
        stmt.and_where(Expr::col(upstream_requests::Column::CredentialId).is_not_null());
    }
    ManagedIndex::new(
        "upstream_requests",
        "idx-upstream-credential-at-trace",
        stmt.take(),
    )
}

fn downstream_user_at_trace_index(partial: bool) -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-downstream-user-at-trace")
        .table(downstream_requests::Entity)
        .col(downstream_requests::Column::UserId)
        .col((downstream_requests::Column::At, IndexOrder::Desc))
        .col((downstream_requests::Column::TraceId, IndexOrder::Desc))
        .if_not_exists();
    if partial {
        stmt.and_where(Expr::col(downstream_requests::Column::UserId).is_not_null());
    }
    ManagedIndex::new(
        "downstream_requests",
        "idx-downstream-user-at-trace",
        stmt.take(),
    )
}

fn downstream_user_key_at_trace_index(partial: bool) -> ManagedIndex {
    let mut stmt = Index::create();
    stmt.name("idx-downstream-user-key-at-trace")
        .table(downstream_requests::Entity)
        .col(downstream_requests::Column::UserKeyId)
        .col((downstream_requests::Column::At, IndexOrder::Desc))
        .col((downstream_requests::Column::TraceId, IndexOrder::Desc))
        .if_not_exists();
    if partial {
        stmt.and_where(Expr::col(downstream_requests::Column::UserKeyId).is_not_null());
    }
    ManagedIndex::new(
        "downstream_requests",
        "idx-downstream-user-key-at-trace",
        stmt.take(),
    )
}

pub(super) async fn mysql_index_exists<C>(
    db: &C,
    table_name: &str,
    index_name: &str,
) -> Result<bool, DbErr>
where
    C: ConnectionTrait,
{
    let statistics = Alias::new("statistics");
    let stmt = Query::select()
        .expr(Expr::val(1))
        .from((Alias::new("information_schema"), statistics.clone()))
        .and_where(
            Expr::col((statistics.clone(), Alias::new("table_schema")))
                .eq(Expr::cust("DATABASE()")),
        )
        .and_where(Expr::col((statistics.clone(), Alias::new("table_name"))).eq(table_name))
        .and_where(Expr::col((statistics, Alias::new("index_name"))).eq(index_name))
        .limit(1)
        .to_owned();
    Ok(db.query_one(&stmt).await?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::sea_query::{MysqlQueryBuilder, PostgresQueryBuilder, SqliteQueryBuilder};

    #[test]
    fn common_indexes_render_for_sqlite() {
        let sql = common_indexes()
            .into_iter()
            .map(|item| item.statement.to_string(SqliteQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            sql.iter()
                .any(|stmt| stmt.contains("idx-usages-at-trace") && stmt.contains("\"at\" DESC"))
        );
        assert!(
            sql.iter()
                .any(|stmt| stmt.contains("idx-providers-channel") && stmt.contains("\"channel\""))
        );
    }

    #[test]
    fn partial_indexes_render_where_clause_for_postgres() {
        let sql = partial_filter_indexes()
            .into_iter()
            .map(|item| item.statement.to_string(PostgresQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            sql.iter()
                .any(|stmt| stmt.contains("idx-usages-user-at-trace")
                    && stmt.contains("WHERE \"user_id\" IS NOT NULL"))
        );
        assert!(
            sql.iter()
                .any(|stmt| stmt.contains("idx-upstream-provider-at-trace")
                    && stmt.contains("WHERE \"provider_id\" IS NOT NULL"))
        );
    }

    #[test]
    fn full_indexes_render_without_where_clause_for_mysql() {
        let sql = full_filter_indexes()
            .into_iter()
            .map(|item| item.statement.to_string(MysqlQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            sql.iter().any(
                |stmt| stmt.contains("idx-downstream-user-at-trace") && !stmt.contains("WHERE")
            )
        );
        assert!(
            sql.iter()
                .any(|stmt| stmt.contains("idx-usages-model-at-trace") && stmt.contains("`model`"))
        );
    }
}
