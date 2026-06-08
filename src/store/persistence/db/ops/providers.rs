//! Provider ops for the `db` backend.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{Provider, ProviderInput};

use super::super::entities::provider;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn to_record(m: provider::Model) -> anyhow::Result<Provider> {
    Ok(Provider {
        id: m.id,
        name: m.name,
        channel: m.channel,
        label: m.label,
        settings_json: serde_json::from_str(&m.settings_json)?,
        credential_strategy: m.credential_strategy,
        enabled: m.enabled,
        created_at: m.created_at,
        updated_at: m.updated_at,
    })
}

pub async fn list(conn: &DatabaseConnection) -> anyhow::Result<Vec<Provider>> {
    provider::Entity::find()
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect()
}

pub async fn get(conn: &DatabaseConnection, id: i64) -> anyhow::Result<Option<Provider>> {
    provider::Entity::find_by_id(id)
        .one(conn)
        .await?
        .map(to_record)
        .transpose()
}

pub async fn get_by_name(
    conn: &DatabaseConnection,
    name: &str,
) -> anyhow::Result<Option<Provider>> {
    provider::Entity::find()
        .filter(provider::Column::Name.eq(name))
        .one(conn)
        .await?
        .map(to_record)
        .transpose()
}

pub async fn upsert(conn: &DatabaseConnection, input: ProviderInput) -> anyhow::Result<Provider> {
    let now = now_secs();
    let settings = serde_json::to_string(&input.settings_json)?;

    let model = match input.id {
        Some(id) => {
            let existing = provider::Entity::find_by_id(id)
                .one(conn)
                .await?
                .ok_or_else(|| anyhow::anyhow!("provider not found: {id}"))?;
            let mut am: provider::ActiveModel = existing.into();
            am.name = Set(input.name);
            am.channel = Set(input.channel);
            am.label = Set(input.label);
            am.settings_json = Set(settings);
            am.credential_strategy = Set(input.credential_strategy);
            am.enabled = Set(input.enabled);
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            let am = provider::ActiveModel {
                id: NotSet,
                name: Set(input.name),
                channel: Set(input.channel),
                label: Set(input.label),
                settings_json: Set(settings),
                credential_strategy: Set(input.credential_strategy),
                enabled: Set(input.enabled),
                created_at: Set(now),
                updated_at: Set(now),
            };
            am.insert(conn).await?
        }
    };

    to_record(model)
}

pub async fn delete(conn: &DatabaseConnection, id: i64) -> anyhow::Result<bool> {
    let res = provider::Entity::delete_by_id(id).exec(conn).await?;
    Ok(res.rows_affected > 0)
}
