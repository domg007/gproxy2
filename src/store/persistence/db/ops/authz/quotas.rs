//! Quota ops for the `db` backend. Unique per `(scope, scope_id)`.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{Quota, QuotaInput, Scope};

use crate::store::persistence::db::entities::authz::quota;

fn to_record(m: quota::Model) -> anyhow::Result<Quota> {
    Ok(Quota {
        id: m.id,
        scope: Scope::parse(&m.scope)?,
        scope_id: m.scope_id,
        quota_total: m.quota_total.parse::<rust_decimal::Decimal>()?,
        cost_used: m.cost_used.parse::<rust_decimal::Decimal>()?,
        created_at: m.created_at,
        updated_at: m.updated_at,
    })
}

pub async fn get(
    conn: &DatabaseConnection,
    scope: Scope,
    scope_id: i64,
) -> anyhow::Result<Option<Quota>> {
    quota::Entity::find()
        .filter(quota::Column::Scope.eq(scope.as_str()))
        .filter(quota::Column::ScopeId.eq(scope_id))
        .one(conn)
        .await?
        .map(to_record)
        .transpose()
}

pub async fn upsert(conn: &DatabaseConnection, input: QuotaInput) -> anyhow::Result<Quota> {
    let now = crate::store::persistence::db::ops::now_secs();

    // Enforce uniqueness on (scope, scope_id): a row for this scope must be the
    // same record we are updating (if any).
    if let Some(existing) = quota::Entity::find()
        .filter(quota::Column::Scope.eq(input.scope.as_str()))
        .filter(quota::Column::ScopeId.eq(input.scope_id))
        .one(conn)
        .await?
        && Some(existing.id) != input.id
    {
        anyhow::bail!(
            "quota already exists for scope {}:{}",
            input.scope.as_str(),
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
            am.scope = Set(input.scope.as_str().to_owned());
            am.scope_id = Set(input.scope_id);
            am.quota_total = Set(input.quota_total.to_string());
            am.cost_used = Set(input.cost_used.to_string());
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            quota::ActiveModel {
                id: NotSet,
                scope: Set(input.scope.as_str().to_owned()),
                scope_id: Set(input.scope_id),
                quota_total: Set(input.quota_total.to_string()),
                cost_used: Set(input.cost_used.to_string()),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(conn)
            .await?
        }
    };

    to_record(model)
}

pub async fn delete(conn: &DatabaseConnection, id: i64) -> anyhow::Result<bool> {
    let res = quota::Entity::delete_by_id(id).exec(conn).await?;
    Ok(res.rows_affected > 0)
}

pub async fn delete_by_scope(
    conn: &DatabaseConnection,
    scope: Scope,
    scope_id: i64,
) -> anyhow::Result<()> {
    quota::Entity::delete_many()
        .filter(quota::Column::Scope.eq(scope.as_str()))
        .filter(quota::Column::ScopeId.eq(scope_id))
        .exec(conn)
        .await?;
    Ok(())
}
