//! Rewrite-rule ops for the `db` backend. `value_json` /
//! `filter_operation_keys` round-trip through serialized JSON text.

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{RewriteRule, RewriteRuleInput};

use crate::store::persistence::db::entities::rules::rewrite_rule;

fn to_record(m: rewrite_rule::Model) -> anyhow::Result<RewriteRule> {
    Ok(RewriteRule {
        id: m.id,
        provider_id: m.provider_id,
        path: m.path,
        action: m.action,
        value_json: m.value_json.map(|s| serde_json::from_str(&s)).transpose()?,
        filter_model_pattern: m.filter_model_pattern,
        filter_operation_keys: m
            .filter_operation_keys
            .map(|s| serde_json::from_str(&s))
            .transpose()?,
        sort_order: m.sort_order,
        enabled: m.enabled,
        created_at: m.created_at,
        updated_at: m.updated_at,
    })
}

pub async fn list(conn: &DatabaseConnection, provider_id: i64) -> anyhow::Result<Vec<RewriteRule>> {
    rewrite_rule::Entity::find()
        .filter(rewrite_rule::Column::ProviderId.eq(provider_id))
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect()
}

pub async fn get(conn: &DatabaseConnection, id: i64) -> anyhow::Result<Option<RewriteRule>> {
    rewrite_rule::Entity::find_by_id(id)
        .one(conn)
        .await?
        .map(to_record)
        .transpose()
}

pub async fn upsert(
    conn: &DatabaseConnection,
    input: RewriteRuleInput,
) -> anyhow::Result<RewriteRule> {
    let now = crate::store::persistence::db::ops::now_secs();
    let value = input
        .value_json
        .map(|v| serde_json::to_string(&v))
        .transpose()?;
    let filter_keys = input
        .filter_operation_keys
        .map(|v| serde_json::to_string(&v))
        .transpose()?;

    let model = match input.id {
        Some(id) => {
            let existing = rewrite_rule::Entity::find_by_id(id)
                .one(conn)
                .await?
                .ok_or_else(|| anyhow::anyhow!("rewrite rule not found: {id}"))?;
            let mut am: rewrite_rule::ActiveModel = existing.into();
            am.provider_id = Set(input.provider_id);
            am.path = Set(input.path);
            am.action = Set(input.action);
            am.value_json = Set(value);
            am.filter_model_pattern = Set(input.filter_model_pattern);
            am.filter_operation_keys = Set(filter_keys);
            am.sort_order = Set(input.sort_order);
            am.enabled = Set(input.enabled);
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            rewrite_rule::ActiveModel {
                id: NotSet,
                provider_id: Set(input.provider_id),
                path: Set(input.path),
                action: Set(input.action),
                value_json: Set(value),
                filter_model_pattern: Set(input.filter_model_pattern),
                filter_operation_keys: Set(filter_keys),
                sort_order: Set(input.sort_order),
                enabled: Set(input.enabled),
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
    let res = rewrite_rule::Entity::delete_by_id(id).exec(conn).await?;
    Ok(res.rows_affected > 0)
}

pub async fn delete_by_provider(conn: &DatabaseConnection, provider_id: i64) -> anyhow::Result<()> {
    rewrite_rule::Entity::delete_many()
        .filter(rewrite_rule::Column::ProviderId.eq(provider_id))
        .exec(conn)
        .await?;
    Ok(())
}
