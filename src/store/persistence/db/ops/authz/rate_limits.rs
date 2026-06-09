//! Rate-limit ops for the `db` backend.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{RateLimit, RateLimitInput};

use crate::store::persistence::db::entities::authz::rate_limit;

fn to_record(m: rate_limit::Model) -> RateLimit {
    RateLimit {
        id: m.id,
        scope: m.scope,
        scope_id: m.scope_id,
        route_pattern: m.route_pattern,
        rpm: m.rpm,
        rpd: m.rpd,
        total_tokens: m.total_tokens,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

pub async fn list(
    conn: &DatabaseConnection,
    scope: &str,
    scope_id: i64,
) -> anyhow::Result<Vec<RateLimit>> {
    Ok(rate_limit::Entity::find()
        .filter(rate_limit::Column::Scope.eq(scope))
        .filter(rate_limit::Column::ScopeId.eq(scope_id))
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect())
}

pub async fn upsert(conn: &DatabaseConnection, input: RateLimitInput) -> anyhow::Result<RateLimit> {
    let now = crate::store::persistence::db::ops::now_secs();

    let model = match input.id {
        Some(id) => {
            let existing = rate_limit::Entity::find_by_id(id)
                .one(conn)
                .await?
                .ok_or_else(|| anyhow::anyhow!("rate limit not found: {id}"))?;
            let mut am: rate_limit::ActiveModel = existing.into();
            am.scope = Set(input.scope);
            am.scope_id = Set(input.scope_id);
            am.route_pattern = Set(input.route_pattern);
            am.rpm = Set(input.rpm);
            am.rpd = Set(input.rpd);
            am.total_tokens = Set(input.total_tokens);
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            rate_limit::ActiveModel {
                id: NotSet,
                scope: Set(input.scope),
                scope_id: Set(input.scope_id),
                route_pattern: Set(input.route_pattern),
                rpm: Set(input.rpm),
                rpd: Set(input.rpd),
                total_tokens: Set(input.total_tokens),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(conn)
            .await?
        }
    };

    Ok(to_record(model))
}

pub async fn delete(conn: &DatabaseConnection, id: i64) -> anyhow::Result<bool> {
    let res = rate_limit::Entity::delete_by_id(id).exec(conn).await?;
    Ok(res.rows_affected > 0)
}

pub async fn delete_by_scope(
    conn: &DatabaseConnection,
    scope: &str,
    scope_id: i64,
) -> anyhow::Result<()> {
    rate_limit::Entity::delete_many()
        .filter(rate_limit::Column::Scope.eq(scope))
        .filter(rate_limit::Column::ScopeId.eq(scope_id))
        .exec(conn)
        .await?;
    Ok(())
}
