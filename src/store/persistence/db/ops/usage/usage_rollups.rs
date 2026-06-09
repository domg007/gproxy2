//! Usage-rollup ops for the `db` backend (accumulate by dimension bucket).

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
};

use crate::store::persistence::records::{UsageRollup, UsageRollupInput};

use crate::store::persistence::db::entities::usage::usage_rollup;

fn to_record(m: usage_rollup::Model) -> anyhow::Result<UsageRollup> {
    Ok(UsageRollup {
        id: m.id,
        granularity: m.granularity,
        bucket_start: m.bucket_start,
        provider_id: m.provider_id,
        org_id: m.org_id,
        team_id: m.team_id,
        user_id: m.user_id,
        route_name: m.route_name,
        model: m.model,
        requests: m.requests,
        input_tokens: m.input_tokens,
        output_tokens: m.output_tokens,
        cost: m.cost.parse::<rust_decimal::Decimal>()?,
        created_at: m.created_at,
        updated_at: m.updated_at,
    })
}

pub async fn add(
    conn: &DatabaseConnection,
    input: UsageRollupInput,
) -> anyhow::Result<UsageRollup> {
    let now = crate::store::persistence::db::ops::now_secs();

    // Locate the existing bucket matching ALL dimensions (incl. None).
    let existing = usage_rollup::Entity::find()
        .filter(usage_rollup::Column::Granularity.eq(input.granularity.clone()))
        .filter(usage_rollup::Column::BucketStart.eq(input.bucket_start))
        .filter(match input.provider_id {
            Some(v) => usage_rollup::Column::ProviderId.eq(v),
            None => usage_rollup::Column::ProviderId.is_null(),
        })
        .filter(match input.org_id {
            Some(v) => usage_rollup::Column::OrgId.eq(v),
            None => usage_rollup::Column::OrgId.is_null(),
        })
        .filter(match input.team_id {
            Some(v) => usage_rollup::Column::TeamId.eq(v),
            None => usage_rollup::Column::TeamId.is_null(),
        })
        .filter(match input.user_id {
            Some(v) => usage_rollup::Column::UserId.eq(v),
            None => usage_rollup::Column::UserId.is_null(),
        })
        .filter(match input.route_name.clone() {
            Some(v) => usage_rollup::Column::RouteName.eq(v),
            None => usage_rollup::Column::RouteName.is_null(),
        })
        .filter(match input.model.clone() {
            Some(v) => usage_rollup::Column::Model.eq(v),
            None => usage_rollup::Column::Model.is_null(),
        })
        .one(conn)
        .await?;

    let model = match existing {
        Some(existing) => {
            let requests = existing.requests + input.requests;
            let input_tokens = existing.input_tokens + input.input_tokens;
            let output_tokens = existing.output_tokens + input.output_tokens;
            let cost = existing.cost.parse::<rust_decimal::Decimal>()? + input.cost;
            let mut am: usage_rollup::ActiveModel = existing.into();
            am.requests = Set(requests);
            am.input_tokens = Set(input_tokens);
            am.output_tokens = Set(output_tokens);
            am.cost = Set(cost.to_string());
            am.updated_at = Set(now);
            am.update(conn).await?
        }
        None => {
            usage_rollup::ActiveModel {
                id: NotSet,
                granularity: Set(input.granularity),
                bucket_start: Set(input.bucket_start),
                provider_id: Set(input.provider_id),
                org_id: Set(input.org_id),
                team_id: Set(input.team_id),
                user_id: Set(input.user_id),
                route_name: Set(input.route_name),
                model: Set(input.model),
                requests: Set(input.requests),
                input_tokens: Set(input.input_tokens),
                output_tokens: Set(input.output_tokens),
                cost: Set(input.cost.to_string()),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(conn)
            .await?
        }
    };

    to_record(model)
}

pub async fn list(
    conn: &DatabaseConnection,
    granularity: &str,
    from: i64,
    to: i64,
) -> anyhow::Result<Vec<UsageRollup>> {
    usage_rollup::Entity::find()
        .filter(usage_rollup::Column::Granularity.eq(granularity))
        .filter(usage_rollup::Column::BucketStart.gte(from))
        .filter(usage_rollup::Column::BucketStart.lte(to))
        .order_by_asc(usage_rollup::Column::BucketStart)
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect()
}
