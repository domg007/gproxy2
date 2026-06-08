//! Beta-header ops for the `db` backend.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{BetaHeader, BetaHeaderInput};

use crate::store::persistence::db::entities::rules::beta_header;

fn to_record(m: beta_header::Model) -> BetaHeader {
    BetaHeader {
        id: m.id,
        provider_id: m.provider_id,
        token: m.token,
        sort_order: m.sort_order,
        enabled: m.enabled,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

pub async fn list(conn: &DatabaseConnection, provider_id: i64) -> anyhow::Result<Vec<BetaHeader>> {
    Ok(beta_header::Entity::find()
        .filter(beta_header::Column::ProviderId.eq(provider_id))
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect())
}

pub async fn get(conn: &DatabaseConnection, id: i64) -> anyhow::Result<Option<BetaHeader>> {
    Ok(beta_header::Entity::find_by_id(id)
        .one(conn)
        .await?
        .map(to_record))
}

pub async fn upsert(
    conn: &DatabaseConnection,
    input: BetaHeaderInput,
) -> anyhow::Result<BetaHeader> {
    let now = crate::store::persistence::db::ops::now_secs();

    let model = match input.id {
        Some(id) => {
            let existing = beta_header::Entity::find_by_id(id)
                .one(conn)
                .await?
                .ok_or_else(|| anyhow::anyhow!("beta header not found: {id}"))?;
            let mut am: beta_header::ActiveModel = existing.into();
            am.provider_id = Set(input.provider_id);
            am.token = Set(input.token);
            am.sort_order = Set(input.sort_order);
            am.enabled = Set(input.enabled);
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            beta_header::ActiveModel {
                id: NotSet,
                provider_id: Set(input.provider_id),
                token: Set(input.token),
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
    let res = beta_header::Entity::delete_by_id(id).exec(conn).await?;
    Ok(res.rows_affected > 0)
}

pub async fn delete_by_provider(conn: &DatabaseConnection, provider_id: i64) -> anyhow::Result<()> {
    beta_header::Entity::delete_many()
        .filter(beta_header::Column::ProviderId.eq(provider_id))
        .exec(conn)
        .await?;
    Ok(())
}
