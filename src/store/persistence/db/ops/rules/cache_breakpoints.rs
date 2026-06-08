//! Cache-breakpoint ops for the `db` backend.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{CacheBreakpoint, CacheBreakpointInput};

use crate::store::persistence::db::entities::rules::cache_breakpoint;

fn to_record(m: cache_breakpoint::Model) -> CacheBreakpoint {
    CacheBreakpoint {
        id: m.id,
        provider_id: m.provider_id,
        target: m.target,
        position: m.position,
        index: m.index,
        ttl: m.ttl,
        sort_order: m.sort_order,
        enabled: m.enabled,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

pub async fn list(
    conn: &DatabaseConnection,
    provider_id: i64,
) -> anyhow::Result<Vec<CacheBreakpoint>> {
    Ok(cache_breakpoint::Entity::find()
        .filter(cache_breakpoint::Column::ProviderId.eq(provider_id))
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect())
}

pub async fn get(conn: &DatabaseConnection, id: i64) -> anyhow::Result<Option<CacheBreakpoint>> {
    Ok(cache_breakpoint::Entity::find_by_id(id)
        .one(conn)
        .await?
        .map(to_record))
}

pub async fn upsert(
    conn: &DatabaseConnection,
    input: CacheBreakpointInput,
) -> anyhow::Result<CacheBreakpoint> {
    let now = crate::store::persistence::db::ops::now_secs();

    let model = match input.id {
        Some(id) => {
            let existing = cache_breakpoint::Entity::find_by_id(id)
                .one(conn)
                .await?
                .ok_or_else(|| anyhow::anyhow!("cache breakpoint not found: {id}"))?;
            let mut am: cache_breakpoint::ActiveModel = existing.into();
            am.provider_id = Set(input.provider_id);
            am.target = Set(input.target);
            am.position = Set(input.position);
            am.index = Set(input.index);
            am.ttl = Set(input.ttl);
            am.sort_order = Set(input.sort_order);
            am.enabled = Set(input.enabled);
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            cache_breakpoint::ActiveModel {
                id: NotSet,
                provider_id: Set(input.provider_id),
                target: Set(input.target),
                position: Set(input.position),
                index: Set(input.index),
                ttl: Set(input.ttl),
                sort_order: Set(input.sort_order),
                enabled: Set(input.enabled),
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
    let res = cache_breakpoint::Entity::delete_by_id(id)
        .exec(conn)
        .await?;
    Ok(res.rows_affected > 0)
}

pub async fn delete_by_provider(conn: &DatabaseConnection, provider_id: i64) -> anyhow::Result<()> {
    cache_breakpoint::Entity::delete_many()
        .filter(cache_breakpoint::Column::ProviderId.eq(provider_id))
        .exec(conn)
        .await?;
    Ok(())
}
