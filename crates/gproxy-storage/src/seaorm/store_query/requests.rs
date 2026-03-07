use sea_orm::{
    ColumnTrait, Condition, DbErr, EntityTrait, FromQueryResult, Order, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::Value;
use time::OffsetDateTime;

use super::super::SeaOrmStorage;
use super::super::entities::{downstream_requests, upstream_requests};
use super::helpers::{apply_desc_cursor, unix_ms_to_offset_datetime};
use crate::query::{
    DownstreamRequestQuery, DownstreamRequestQueryRow, RequestQueryCount, Scope,
    UpstreamRequestQuery, UpstreamRequestQueryRow,
};

impl SeaOrmStorage {
    pub async fn query_upstream_requests(
        &self,
        query: &UpstreamRequestQuery,
    ) -> Result<Vec<UpstreamRequestQueryRow>, DbErr> {
        let include_body = query.include_body.unwrap_or(false);
        if include_body {
            let mut stmt = upstream_requests::Entity::find()
                .order_by(upstream_requests::Column::At, Order::Desc)
                .order_by(upstream_requests::Column::TraceId, Order::Desc);
            stmt = apply_upstream_request_filters(stmt, query);
            stmt = apply_desc_cursor(
                stmt,
                upstream_requests::Column::At,
                upstream_requests::Column::TraceId,
                query.cursor_at_unix_ms,
                query.cursor_trace_id,
            );
            if let Some(offset) = query.offset
                && offset > 0
            {
                stmt = stmt.offset(offset);
            }
            if let Some(limit) = query.limit
                && limit > 0
            {
                stmt = stmt.limit(limit);
            }
            let rows = stmt.all(self.connection()).await?;
            return Ok(rows
                .into_iter()
                .map(|row| UpstreamRequestQueryRow {
                    trace_id: row.trace_id,
                    downstream_trace_id: row.downstream_trace_id,
                    at: row.at,
                    internal: row.internal,
                    provider_id: row.provider_id,
                    credential_id: row.credential_id,
                    request_method: row.request_method,
                    request_headers_json: row.request_headers_json,
                    request_url: row.request_url,
                    request_body: row.request_body,
                    response_status: row.response_status,
                    response_headers_json: row.response_headers_json,
                    response_body: row.response_body,
                    created_at: row.created_at,
                })
                .collect());
        }

        let mut stmt = upstream_requests::Entity::find()
            .select_only()
            .column(upstream_requests::Column::TraceId)
            .column(upstream_requests::Column::DownstreamTraceId)
            .column(upstream_requests::Column::At)
            .column(upstream_requests::Column::Internal)
            .column(upstream_requests::Column::ProviderId)
            .column(upstream_requests::Column::CredentialId)
            .column(upstream_requests::Column::RequestMethod)
            .column(upstream_requests::Column::RequestHeadersJson)
            .column(upstream_requests::Column::RequestUrl)
            .column(upstream_requests::Column::ResponseStatus)
            .column(upstream_requests::Column::ResponseHeadersJson)
            .column(upstream_requests::Column::CreatedAt)
            .order_by(upstream_requests::Column::At, Order::Desc)
            .order_by(upstream_requests::Column::TraceId, Order::Desc);
        stmt = apply_upstream_request_filters(stmt, query);
        stmt = apply_desc_cursor(
            stmt,
            upstream_requests::Column::At,
            upstream_requests::Column::TraceId,
            query.cursor_at_unix_ms,
            query.cursor_trace_id,
        );
        if let Some(offset) = query.offset
            && offset > 0
        {
            stmt = stmt.offset(offset);
        }
        if let Some(limit) = query.limit
            && limit > 0
        {
            stmt = stmt.limit(limit);
        }
        let rows = stmt
            .into_model::<UpstreamRequestQueryRowNoBodyModel>()
            .all(self.connection())
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn query_downstream_requests(
        &self,
        query: &DownstreamRequestQuery,
    ) -> Result<Vec<DownstreamRequestQueryRow>, DbErr> {
        let include_body = query.include_body.unwrap_or(false);
        if include_body {
            let mut stmt = downstream_requests::Entity::find()
                .order_by(downstream_requests::Column::At, Order::Desc)
                .order_by(downstream_requests::Column::TraceId, Order::Desc);
            stmt = apply_downstream_request_filters(stmt, query);
            stmt = apply_desc_cursor(
                stmt,
                downstream_requests::Column::At,
                downstream_requests::Column::TraceId,
                query.cursor_at_unix_ms,
                query.cursor_trace_id,
            );
            if let Some(offset) = query.offset
                && offset > 0
            {
                stmt = stmt.offset(offset);
            }
            if let Some(limit) = query.limit
                && limit > 0
            {
                stmt = stmt.limit(limit);
            }
            let rows = stmt.all(self.connection()).await?;
            return Ok(rows
                .into_iter()
                .map(|row| DownstreamRequestQueryRow {
                    trace_id: row.trace_id,
                    at: row.at,
                    internal: row.internal,
                    user_id: row.user_id,
                    user_key_id: row.user_key_id,
                    request_method: row.request_method,
                    request_headers_json: row.request_headers_json,
                    request_path: row.request_path,
                    request_query: row.request_query,
                    request_body: row.request_body,
                    response_status: row.response_status,
                    response_headers_json: row.response_headers_json,
                    response_body: row.response_body,
                    created_at: row.created_at,
                })
                .collect());
        }

        let mut stmt = downstream_requests::Entity::find()
            .select_only()
            .column(downstream_requests::Column::TraceId)
            .column(downstream_requests::Column::At)
            .column(downstream_requests::Column::Internal)
            .column(downstream_requests::Column::UserId)
            .column(downstream_requests::Column::UserKeyId)
            .column(downstream_requests::Column::RequestMethod)
            .column(downstream_requests::Column::RequestHeadersJson)
            .column(downstream_requests::Column::RequestPath)
            .column(downstream_requests::Column::RequestQuery)
            .column(downstream_requests::Column::ResponseStatus)
            .column(downstream_requests::Column::ResponseHeadersJson)
            .column(downstream_requests::Column::CreatedAt)
            .order_by(downstream_requests::Column::At, Order::Desc)
            .order_by(downstream_requests::Column::TraceId, Order::Desc);
        stmt = apply_downstream_request_filters(stmt, query);
        stmt = apply_desc_cursor(
            stmt,
            downstream_requests::Column::At,
            downstream_requests::Column::TraceId,
            query.cursor_at_unix_ms,
            query.cursor_trace_id,
        );
        if let Some(offset) = query.offset
            && offset > 0
        {
            stmt = stmt.offset(offset);
        }
        if let Some(limit) = query.limit
            && limit > 0
        {
            stmt = stmt.limit(limit);
        }
        let rows = stmt
            .into_model::<DownstreamRequestQueryRowNoBodyModel>()
            .all(self.connection())
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn count_upstream_requests(
        &self,
        query: &UpstreamRequestQuery,
    ) -> Result<RequestQueryCount, DbErr> {
        let stmt = apply_upstream_request_filters(upstream_requests::Entity::find(), query);
        let count = stmt.count(self.connection()).await?;
        Ok(RequestQueryCount { count })
    }

    pub async fn count_downstream_requests(
        &self,
        query: &DownstreamRequestQuery,
    ) -> Result<RequestQueryCount, DbErr> {
        let stmt = apply_downstream_request_filters(downstream_requests::Entity::find(), query);
        let count = stmt.count(self.connection()).await?;
        Ok(RequestQueryCount { count })
    }
}

fn apply_upstream_request_filters<S>(stmt: S, query: &UpstreamRequestQuery) -> S
where
    S: QueryFilter,
{
    let mut condition = Condition::all();
    if let Scope::Eq(trace_id) = query.trace_id {
        condition = condition.add(upstream_requests::Column::TraceId.eq(trace_id));
    }
    if let Scope::Eq(provider_id) = query.provider_id {
        condition = condition.add(upstream_requests::Column::ProviderId.eq(provider_id));
    }
    if let Scope::Eq(credential_id) = query.credential_id {
        condition = condition.add(upstream_requests::Column::CredentialId.eq(credential_id));
    }
    if let Some(url_contains) = query.request_url_contains.as_deref() {
        let needle = url_contains.trim();
        if !needle.is_empty() {
            condition = condition.add(upstream_requests::Column::RequestUrl.contains(needle));
        }
    }
    if let Some(from_unix_ms) = query.from_unix_ms
        && let Ok(from) = unix_ms_to_offset_datetime(from_unix_ms)
    {
        condition = condition.add(upstream_requests::Column::At.gte(from));
    }
    if let Some(to_unix_ms) = query.to_unix_ms
        && let Ok(to) = unix_ms_to_offset_datetime(to_unix_ms)
    {
        condition = condition.add(upstream_requests::Column::At.lt(to));
    }
    stmt.filter(condition)
}

fn apply_downstream_request_filters<S>(stmt: S, query: &DownstreamRequestQuery) -> S
where
    S: QueryFilter,
{
    let mut condition = Condition::all();
    if let Scope::Eq(trace_id) = query.trace_id {
        condition = condition.add(downstream_requests::Column::TraceId.eq(trace_id));
    }
    if let Scope::Eq(user_id) = query.user_id {
        condition = condition.add(downstream_requests::Column::UserId.eq(user_id));
    }
    if let Scope::Eq(user_key_id) = query.user_key_id {
        condition = condition.add(downstream_requests::Column::UserKeyId.eq(user_key_id));
    }
    if let Some(path_contains) = query.request_path_contains.as_deref() {
        let needle = path_contains.trim();
        if !needle.is_empty() {
            condition = condition.add(downstream_requests::Column::RequestPath.contains(needle));
        }
    }
    if let Some(from_unix_ms) = query.from_unix_ms
        && let Ok(from) = unix_ms_to_offset_datetime(from_unix_ms)
    {
        condition = condition.add(downstream_requests::Column::At.gte(from));
    }
    if let Some(to_unix_ms) = query.to_unix_ms
        && let Ok(to) = unix_ms_to_offset_datetime(to_unix_ms)
    {
        condition = condition.add(downstream_requests::Column::At.lt(to));
    }
    stmt.filter(condition)
}

#[derive(Debug, Clone, FromQueryResult)]
struct UpstreamRequestQueryRowNoBodyModel {
    pub trace_id: i64,
    pub downstream_trace_id: Option<i64>,
    pub at: OffsetDateTime,
    pub internal: bool,
    pub provider_id: Option<i64>,
    pub credential_id: Option<i64>,
    pub request_method: String,
    pub request_headers_json: Value,
    pub request_url: Option<String>,
    pub response_status: Option<i32>,
    pub response_headers_json: Value,
    pub created_at: OffsetDateTime,
}

impl From<UpstreamRequestQueryRowNoBodyModel> for UpstreamRequestQueryRow {
    fn from(value: UpstreamRequestQueryRowNoBodyModel) -> Self {
        Self {
            trace_id: value.trace_id,
            downstream_trace_id: value.downstream_trace_id,
            at: value.at,
            internal: value.internal,
            provider_id: value.provider_id,
            credential_id: value.credential_id,
            request_method: value.request_method,
            request_headers_json: value.request_headers_json,
            request_url: value.request_url,
            request_body: None,
            response_status: value.response_status,
            response_headers_json: value.response_headers_json,
            response_body: None,
            created_at: value.created_at,
        }
    }
}

#[derive(Debug, Clone, FromQueryResult)]
struct DownstreamRequestQueryRowNoBodyModel {
    pub trace_id: i64,
    pub at: OffsetDateTime,
    pub internal: bool,
    pub user_id: Option<i64>,
    pub user_key_id: Option<i64>,
    pub request_method: String,
    pub request_headers_json: Value,
    pub request_path: String,
    pub request_query: Option<String>,
    pub response_status: Option<i32>,
    pub response_headers_json: Value,
    pub created_at: OffsetDateTime,
}

impl From<DownstreamRequestQueryRowNoBodyModel> for DownstreamRequestQueryRow {
    fn from(value: DownstreamRequestQueryRowNoBodyModel) -> Self {
        Self {
            trace_id: value.trace_id,
            at: value.at,
            internal: value.internal,
            user_id: value.user_id,
            user_key_id: value.user_key_id,
            request_method: value.request_method,
            request_headers_json: value.request_headers_json,
            request_path: value.request_path,
            request_query: value.request_query,
            request_body: None,
            response_status: value.response_status,
            response_headers_json: value.response_headers_json,
            response_body: None,
            created_at: value.created_at,
        }
    }
}
