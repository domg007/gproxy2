//! Sanitize-rule ops for the `db` backend.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{SanitizeRule, SanitizeRuleInput};

use crate::store::persistence::db::entities::rules::sanitize_rule;

fn to_record(m: sanitize_rule::Model) -> SanitizeRule {
    SanitizeRule {
        id: m.id,
        provider_id: m.provider_id,
        pattern: m.pattern,
        replacement: m.replacement,
        sort_order: m.sort_order,
        enabled: m.enabled,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}

pub async fn list(
    conn: &DatabaseConnection,
    provider_id: i64,
) -> anyhow::Result<Vec<SanitizeRule>> {
    Ok(sanitize_rule::Entity::find()
        .filter(sanitize_rule::Column::ProviderId.eq(provider_id))
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect())
}

pub async fn get(conn: &DatabaseConnection, id: i64) -> anyhow::Result<Option<SanitizeRule>> {
    Ok(sanitize_rule::Entity::find_by_id(id)
        .one(conn)
        .await?
        .map(to_record))
}

pub async fn upsert(
    conn: &DatabaseConnection,
    input: SanitizeRuleInput,
) -> anyhow::Result<SanitizeRule> {
    let now = crate::store::persistence::db::ops::now_secs();

    let model = match input.id {
        Some(id) => {
            let existing = sanitize_rule::Entity::find_by_id(id)
                .one(conn)
                .await?
                .ok_or_else(|| anyhow::anyhow!("sanitize rule not found: {id}"))?;
            let mut am: sanitize_rule::ActiveModel = existing.into();
            am.provider_id = Set(input.provider_id);
            am.pattern = Set(input.pattern);
            am.replacement = Set(input.replacement);
            am.sort_order = Set(input.sort_order);
            am.enabled = Set(input.enabled);
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            sanitize_rule::ActiveModel {
                id: NotSet,
                provider_id: Set(input.provider_id),
                pattern: Set(input.pattern),
                replacement: Set(input.replacement),
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
    let res = sanitize_rule::Entity::delete_by_id(id).exec(conn).await?;
    Ok(res.rows_affected > 0)
}

pub async fn delete_by_provider(conn: &DatabaseConnection, provider_id: i64) -> anyhow::Result<()> {
    sanitize_rule::Entity::delete_many()
        .filter(sanitize_rule::Column::ProviderId.eq(provider_id))
        .exec(conn)
        .await?;
    Ok(())
}
