//! Route ops for the `db` backend.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{Route, RouteInput};

use crate::store::persistence::db::entities::routing::route;

fn to_record(m: route::Model) -> Route {
    Route {
        id: m.id,
        name: m.name,
        strategy: m.strategy,
        enabled: m.enabled,
        description: m.description,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

pub async fn list(conn: &DatabaseConnection) -> anyhow::Result<Vec<Route>> {
    Ok(route::Entity::find()
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect())
}

pub async fn get(conn: &DatabaseConnection, id: i64) -> anyhow::Result<Option<Route>> {
    Ok(route::Entity::find_by_id(id)
        .one(conn)
        .await?
        .map(to_record))
}

pub async fn get_by_name(conn: &DatabaseConnection, name: &str) -> anyhow::Result<Option<Route>> {
    Ok(route::Entity::find()
        .filter(route::Column::Name.eq(name))
        .one(conn)
        .await?
        .map(to_record))
}

pub async fn upsert(conn: &DatabaseConnection, input: RouteInput) -> anyhow::Result<Route> {
    let now = crate::store::persistence::db::ops::now_secs();

    let model = match input.id {
        Some(id) => {
            let existing = route::Entity::find_by_id(id)
                .one(conn)
                .await?
                .ok_or_else(|| anyhow::anyhow!("route not found: {id}"))?;
            let mut am: route::ActiveModel = existing.into();
            am.name = Set(input.name);
            am.strategy = Set(input.strategy);
            am.enabled = Set(input.enabled);
            am.description = Set(input.description);
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            route::ActiveModel {
                id: NotSet,
                name: Set(input.name),
                strategy: Set(input.strategy),
                enabled: Set(input.enabled),
                description: Set(input.description),
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
    // cascade: members and aliases of this route.
    super::route_members::delete_by_route(conn, id).await?;
    super::aliases::delete_by_route(conn, id).await?;

    let res = route::Entity::delete_by_id(id).exec(conn).await?;
    Ok(res.rows_affected > 0)
}
