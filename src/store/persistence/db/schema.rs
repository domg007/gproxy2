//! Schema creation on connect. Derives `CREATE TABLE IF NOT EXISTS` from the
//! SeaORM entities for whatever dialect the connection uses (single source of
//! truth = the entity definitions; no separate migration crate yet).

use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait, Schema};

use super::entities::authz::{quota, rate_limit, route_permission};
use super::entities::identity::{org, team, user, user_key};
use super::entities::logs::{downstream_request, upstream_request};
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
