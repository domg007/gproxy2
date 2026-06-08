//! Downstream-request log ops for the `db` backend (append-only).

use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::store::persistence::records::{DownstreamRequest, DownstreamRequestInput};

use crate::store::persistence::db::entities::usage::downstream_request;

fn to_record(m: downstream_request::Model) -> anyhow::Result<DownstreamRequest> {
    Ok(DownstreamRequest {
        id: m.id,
        request_id: m.request_id,
        at: m.at,
        method: m.method,
        path: m.path,
        query: m.query,
        status: m.status,
        headers_json: m
            .headers_json
            .map(|s| serde_json::from_str(&s))
            .transpose()?,
        body: m.body,
        created_at: m.created_at,
        updated_at: m.updated_at,
    })
}

pub async fn append(
    conn: &DatabaseConnection,
    input: DownstreamRequestInput,
) -> anyhow::Result<DownstreamRequest> {
    let now = crate::store::persistence::db::ops::now_secs();
    let headers = input
        .headers_json
        .map(|v| serde_json::to_string(&v))
        .transpose()?;

    let model = downstream_request::ActiveModel {
        id: NotSet,
        request_id: Set(input.request_id),
        at: Set(input.at),
        method: Set(input.method),
        path: Set(input.path),
        query: Set(input.query),
        status: Set(input.status),
        headers_json: Set(headers),
        body: Set(input.body),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(conn)
    .await?;

    to_record(model)
}

pub async fn list(
    conn: &DatabaseConnection,
    request_id: &str,
) -> anyhow::Result<Vec<DownstreamRequest>> {
    downstream_request::Entity::find()
        .filter(downstream_request::Column::RequestId.eq(request_id))
        .all(conn)
        .await?
        .into_iter()
        .map(to_record)
        .collect()
}
