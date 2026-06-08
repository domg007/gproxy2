//! System-prelude ops for the `db` backend.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{PreludeSystem, PreludeSystemInput};

use crate::store::persistence::db::entities::rules::prelude_system;

fn to_record(m: prelude_system::Model) -> PreludeSystem {
    PreludeSystem {
        id: m.id,
        provider_id: m.provider_id,
        text: m.text,
        sort_order: m.sort_order,
        enabled: m.enabled,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

pub async fn list(
    conn: &DatabaseConnection,
    provider_id: i64,
) -> anyhow::Result<Vec<PreludeSystem>> {
    Ok(prelude_system::Entity::find()
        .filter(prelude_system::Column::ProviderId.eq(provider_id))
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect())
}

pub async fn get(conn: &DatabaseConnection, id: i64) -> anyhow::Result<Option<PreludeSystem>> {
    Ok(prelude_system::Entity::find_by_id(id)
        .one(conn)
        .await?
        .map(to_record))
}

pub async fn upsert(
    conn: &DatabaseConnection,
    input: PreludeSystemInput,
) -> anyhow::Result<PreludeSystem> {
    let now = crate::store::persistence::db::ops::now_secs();

    let model = match input.id {
        Some(id) => {
            let existing = prelude_system::Entity::find_by_id(id)
                .one(conn)
                .await?
                .ok_or_else(|| anyhow::anyhow!("system prelude not found: {id}"))?;
            let mut am: prelude_system::ActiveModel = existing.into();
            am.provider_id = Set(input.provider_id);
            am.text = Set(input.text);
            am.sort_order = Set(input.sort_order);
            am.enabled = Set(input.enabled);
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            prelude_system::ActiveModel {
                id: NotSet,
                provider_id: Set(input.provider_id),
                text: Set(input.text),
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
    let res = prelude_system::Entity::delete_by_id(id).exec(conn).await?;
    Ok(res.rows_affected > 0)
}

pub async fn delete_by_provider(conn: &DatabaseConnection, provider_id: i64) -> anyhow::Result<()> {
    prelude_system::Entity::delete_many()
        .filter(prelude_system::Column::ProviderId.eq(provider_id))
        .exec(conn)
        .await?;
    Ok(())
}
