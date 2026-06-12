//! Schema creation on connect. Derives `CREATE TABLE IF NOT EXISTS` from the
//! SeaORM entities for whatever dialect the connection uses (single source of
//! truth = the entity definitions; no separate migration crate yet).

use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait, Schema, Statement};

use crate::store::persistence::migrations::{
    BASELINE_VERSION, CREATE_MIGRATIONS_TABLE, SELECT_MAX_VERSION, pending,
};

use super::entities::authz::{quota, rate_limit, route_permission};
use super::entities::identity::{org, team, user, user_key};
use super::entities::logs::{audit_log, downstream_request, upstream_request};
use super::entities::provider::{credential, credential_status, provider, provider_model};
use super::entities::routing::{alias, route, route_member};
use super::entities::settings::instance_setting;
use super::entities::tokenize::tokenizer_vocab;
use super::entities::transform::{provider_rule_set, routing_rule, rule, rule_set};
use super::entities::usage::{usage, usage_rollup};

pub(super) async fn create_all(conn: &DatabaseConnection) -> anyhow::Result<()> {
    let backend = conn.get_database_backend();
    let schema = Schema::new(backend);

    create_table(conn, &schema, provider::Entity).await?;
    create_table(conn, &schema, credential::Entity).await?;
    create_table(conn, &schema, credential_status::Entity).await?;
    create_table(conn, &schema, provider_model::Entity).await?;
    create_table(conn, &schema, route::Entity).await?;
    create_table(conn, &schema, route_member::Entity).await?;
    create_table(conn, &schema, alias::Entity).await?;

    // §8-B2 rules
    create_table(conn, &schema, routing_rule::Entity).await?;
    create_table(conn, &schema, rule_set::Entity).await?;
    create_table(conn, &schema, rule::Entity).await?;
    create_table(conn, &schema, provider_rule_set::Entity).await?;

    // §8-C identity
    create_table(conn, &schema, org::Entity).await?;
    create_table(conn, &schema, team::Entity).await?;
    create_table(conn, &schema, user::Entity).await?;
    create_table(conn, &schema, user_key::Entity).await?;
    create_table(conn, &schema, route_permission::Entity).await?;
    create_table(conn, &schema, rate_limit::Entity).await?;
    create_table(conn, &schema, quota::Entity).await?;

    // §8-D usage
    create_table(conn, &schema, usage::Entity).await?;
    create_table(conn, &schema, usage_rollup::Entity).await?;
    create_table(conn, &schema, downstream_request::Entity).await?;
    create_table(conn, &schema, upstream_request::Entity).await?;
    create_table(conn, &schema, audit_log::Entity).await?;

    // §8-E settings
    create_table(conn, &schema, instance_setting::Entity).await?;

    // §6.3 tokenizer vocabs
    create_table(conn, &schema, tokenizer_vocab::Entity).await?;

    Ok(())
}

async fn create_table<E: EntityTrait>(
    conn: &DatabaseConnection,
    schema: &Schema,
    entity: E,
) -> anyhow::Result<()> {
    let mut stmt = schema.create_table_from_entity(entity);
    stmt.if_not_exists();
    conn.execute(&stmt).await?;
    Ok(())
}

/// Stamp the baseline (if unstamped) and apply any pending ordered migrations.
///
/// Assumes [`create_all`] has already run, so a fresh DB and a DB created by the
/// old auto-create both already hold the baseline tables. We therefore detect
/// the "no `schema_migrations` row" case and stamp [`BASELINE_VERSION`] WITHOUT
/// re-running any creation/destructive step, then apply `version >` baseline
/// migrations in order.
pub(super) async fn run_migrations(conn: &DatabaseConnection) -> anyhow::Result<()> {
    let backend = conn.get_database_backend();

    // Writes go through `execute_unprepared` (raw SQL, dialect-portable). The
    // version read uses `query_one_raw`, which takes a `Statement` by value.
    conn.execute_unprepared(CREATE_MIGRATIONS_TABLE).await?;

    let current = conn
        .query_one_raw(Statement::from_string(backend, SELECT_MAX_VERSION))
        .await?
        .map(|row| row.try_get::<i64>("", "v"))
        .transpose()?
        .unwrap_or(0);

    // Empty table → stamp the baseline the existing create routine just ensured.
    let current = if current == 0 {
        record_version(conn, BASELINE_VERSION).await?;
        BASELINE_VERSION
    } else {
        current
    };

    for m in pending(current) {
        for sql in m.sql {
            conn.execute_unprepared(sql).await?;
        }
        record_version(conn, m.version).await?;
    }
    Ok(())
}

async fn record_version(conn: &DatabaseConnection, version: i64) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    conn.execute_unprepared(&format!(
        "INSERT INTO schema_migrations (version, applied_at) VALUES ({version}, {now})"
    ))
    .await?;
    Ok(())
}
