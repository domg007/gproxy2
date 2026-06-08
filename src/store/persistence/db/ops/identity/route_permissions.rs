//! Route-permission ops for the `db` backend.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{RoutePermission, RoutePermissionInput};

use crate::store::persistence::db::entities::identity::route_permission;

fn to_record(m: route_permission::Model) -> RoutePermission {
    RoutePermission {
        id: m.id,
        scope: m.scope,
        scope_id: m.scope_id,
        route_pattern: m.route_pattern,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

pub async fn list(
    conn: &DatabaseConnection,
    scope: &str,
    scope_id: i64,
) -> anyhow::Result<Vec<RoutePermission>> {
    Ok(route_permission::Entity::find()
        .filter(route_permission::Column::Scope.eq(scope))
        .filter(route_permission::Column::ScopeId.eq(scope_id))
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect())
}

pub async fn upsert(
    conn: &DatabaseConnection,
    input: RoutePermissionInput,
) -> anyhow::Result<RoutePermission> {
    let now = crate::store::persistence::db::ops::now_secs();

    let model = match input.id {
        Some(id) => {
            let existing = route_permission::Entity::find_by_id(id)
                .one(conn)
                .await?
                .ok_or_else(|| anyhow::anyhow!("route permission not found: {id}"))?;
            let mut am: route_permission::ActiveModel = existing.into();
            am.scope = Set(input.scope);
            am.scope_id = Set(input.scope_id);
            am.route_pattern = Set(input.route_pattern);
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            route_permission::ActiveModel {
                id: NotSet,
                scope: Set(input.scope),
                scope_id: Set(input.scope_id),
                route_pattern: Set(input.route_pattern),
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
    let res = route_permission::Entity::delete_by_id(id)
        .exec(conn)
        .await?;
    Ok(res.rows_affected > 0)
}

pub async fn delete_by_scope(
    conn: &DatabaseConnection,
    scope: &str,
    scope_id: i64,
) -> anyhow::Result<()> {
    route_permission::Entity::delete_many()
        .filter(route_permission::Column::Scope.eq(scope))
        .filter(route_permission::Column::ScopeId.eq(scope_id))
        .exec(conn)
        .await?;
    Ok(())
}
