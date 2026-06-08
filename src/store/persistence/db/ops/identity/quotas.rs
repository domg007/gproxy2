//! Quota ops for the `db` backend. Unique per `(scope, scope_id)`.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{Quota, QuotaInput};

use crate::store::persistence::db::entities::identity::quota;

fn to_record(m: quota::Model) -> Quota {
    Quota {
        id: m.id,
        scope: m.scope,
        scope_id: m.scope_id,
        quota_total: m.quota_total,
        cost_used: m.cost_used,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

pub async fn get(
    conn: &DatabaseConnection,
    scope: &str,
    scope_id: i64,
) -> anyhow::Result<Option<Quota>> {
    Ok(quota::Entity::find()
        .filter(quota::Column::Scope.eq(scope))
        .filter(quota::Column::ScopeId.eq(scope_id))
        .one(conn)
        .await?
        .map(to_record))
}

pub async fn upsert(conn: &DatabaseConnection, input: QuotaInput) -> anyhow::Result<Quota> {
    let now = crate::store::persistence::db::ops::now_secs();

    // Enforce uniqueness on (scope, scope_id): a row for this scope must be the
    // same record we are updating (if any).
    if let Some(existing) = quota::Entity::find()
        .filter(quota::Column::Scope.eq(input.scope.clone()))
        .filter(quota::Column::ScopeId.eq(input.scope_id))
        .one(conn)
        .await?
        && Some(existing.id) != input.id
    {
        anyhow::bail!(
            "quota already exists for scope {}:{}",
            input.scope,
            input.scope_id
        );
    }

    let model = match input.id {
        Some(id) => {
            let existing = quota::Entity::find_by_id(id)
                .one(conn)
                .await?
                .ok_or_else(|| anyhow::anyhow!("quota not found: {id}"))?;
            let mut am: quota::ActiveModel = existing.into();
            am.scope = Set(input.scope);
            am.scope_id = Set(input.scope_id);
            am.quota_total = Set(input.quota_total);
            am.cost_used = Set(input.cost_used);
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            quota::ActiveModel {
                id: NotSet,
                scope: Set(input.scope),
                scope_id: Set(input.scope_id),
                quota_total: Set(input.quota_total),
                cost_used: Set(input.cost_used),
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
    let res = quota::Entity::delete_by_id(id).exec(conn).await?;
    Ok(res.rows_affected > 0)
}

pub async fn delete_by_scope(
    conn: &DatabaseConnection,
    scope: &str,
    scope_id: i64,
) -> anyhow::Result<()> {
    quota::Entity::delete_many()
        .filter(quota::Column::Scope.eq(scope))
        .filter(quota::Column::ScopeId.eq(scope_id))
        .exec(conn)
        .await?;
    Ok(())
}
