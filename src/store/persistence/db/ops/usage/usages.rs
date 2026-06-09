//! Usage ops for the `db` backend (append-only).

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, QueryOrder, QuerySelect};

use crate::store::persistence::records::{Usage, UsageInput};

use crate::store::persistence::db::entities::usage::usage;

fn to_record(m: usage::Model) -> anyhow::Result<Usage> {
    Ok(Usage {
        id: m.id,
        request_id: m.request_id,
        at: m.at,
        route_name: m.route_name,
        provider_id: m.provider_id,
        credential_id: m.credential_id,
        org_id: m.org_id,
        team_id: m.team_id,
        user_id: m.user_id,
        user_key_id: m.user_key_id,
        operation: m.operation,
        kind: m.kind,
        model: m.model,
        input_tokens: m.input_tokens,
        output_tokens: m.output_tokens,
        cache_read_tokens: m.cache_read_tokens,
        cache_creation_5m_tokens: m.cache_creation_5m_tokens,
        cache_creation_1h_tokens: m.cache_creation_1h_tokens,
        cost: m.cost.parse::<rust_decimal::Decimal>()?,
        created_at: m.created_at,
        updated_at: m.updated_at,
    })
}

pub async fn append(conn: &DatabaseConnection, input: UsageInput) -> anyhow::Result<Usage> {
    let now = crate::store::persistence::db::ops::now_secs();
    let model = usage::ActiveModel {
        id: NotSet,
        request_id: Set(input.request_id),
        at: Set(input.at),
        route_name: Set(input.route_name),
        provider_id: Set(input.provider_id),
        credential_id: Set(input.credential_id),
        org_id: Set(input.org_id),
        team_id: Set(input.team_id),
        user_id: Set(input.user_id),
        user_key_id: Set(input.user_key_id),
        operation: Set(input.operation),
        kind: Set(input.kind),
        model: Set(input.model),
        input_tokens: Set(input.input_tokens),
        output_tokens: Set(input.output_tokens),
        cache_read_tokens: Set(input.cache_read_tokens),
        cache_creation_5m_tokens: Set(input.cache_creation_5m_tokens),
        cache_creation_1h_tokens: Set(input.cache_creation_1h_tokens),
        cost: Set(input.cost.to_string()),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(conn)
    .await?;
    to_record(model)
}

pub async fn list(conn: &DatabaseConnection, limit: u64) -> anyhow::Result<Vec<Usage>> {
    usage::Entity::find()
        .order_by_desc(usage::Column::Id)
        .limit(limit)
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect()
}
