use sea_orm::*;

use super::helpers::{apply_desc_cursor, unix_ms_to_offset_datetime};
use crate::query::*;
use crate::seaorm::SeaOrmStorage;
use crate::seaorm::entities::*;

/// Request log queries — always hit the database at runtime.
/// Request logs are not cached in memory (too large).
impl SeaOrmStorage {
    pub async fn query_upstream_requests(
        &self,
        query: &UpstreamRequestQuery,
    ) -> Result<Vec<UpstreamRequestQueryRow>, DbErr> {
        let mut select = upstream_requests::Entity::find()
            .order_by_desc(upstream_requests::Column::At)
            .order_by_desc(upstream_requests::Column::TraceId);

        if let Scope::Eq(ref v) = query.trace_id {
            select = select.filter(upstream_requests::Column::TraceId.eq(*v));
        }
        if let Scope::Eq(ref v) = query.provider_id {
            select = select.filter(upstream_requests::Column::ProviderId.eq(*v));
        }
        if let Scope::Eq(ref v) = query.credential_id {
            select = select.filter(upstream_requests::Column::CredentialId.eq(*v));
        }
        if let Some(ref contains) = query.request_url_contains {
            select = select.filter(upstream_requests::Column::RequestUrl.contains(contains));
        }
        if let Some(from) = query.from_unix_ms {
            select =
                select.filter(upstream_requests::Column::At.gte(unix_ms_to_offset_datetime(from)));
        }
        if let Some(to) = query.to_unix_ms {
            select =
                select.filter(upstream_requests::Column::At.lte(unix_ms_to_offset_datetime(to)));
        }
        select = apply_desc_cursor(
            select,
            query.cursor_at_unix_ms,
            query.cursor_trace_id,
            upstream_requests::Column::At,
            upstream_requests::Column::TraceId,
        );
        if let Some(offset) = query.offset {
            select = select.offset(offset);
        }
        if let Some(limit) = query.limit {
            select = select.limit(limit);
        }

        let rows = select.all(&self.db).await?;
        let include_body = query.include_body.unwrap_or(false);
        Ok(rows
            .into_iter()
            .map(|r| UpstreamRequestQueryRow {
                trace_id: r.trace_id,
                downstream_trace_id: r.downstream_trace_id,
                at: r.at,
                internal: r.internal,
                provider_id: r.provider_id,
                credential_id: r.credential_id,
                request_method: r.request_method,
                request_headers_json: r.request_headers_json,
                request_url: r.request_url,
                request_body: if include_body { r.request_body } else { None },
                response_status: r.response_status,
                response_headers_json: r.response_headers_json,
                response_body: if include_body { r.response_body } else { None },
                created_at: r.created_at,
            })
            .collect())
    }

    pub async fn count_upstream_requests(
        &self,
        query: &UpstreamRequestQuery,
    ) -> Result<RequestQueryCount, DbErr> {
        let mut select = upstream_requests::Entity::find();
        if let Scope::Eq(ref v) = query.provider_id {
            select = select.filter(upstream_requests::Column::ProviderId.eq(*v));
        }
        if let Scope::Eq(ref v) = query.credential_id {
            select = select.filter(upstream_requests::Column::CredentialId.eq(*v));
        }
        if let Some(from) = query.from_unix_ms {
            select =
                select.filter(upstream_requests::Column::At.gte(unix_ms_to_offset_datetime(from)));
        }
        if let Some(to) = query.to_unix_ms {
            select =
                select.filter(upstream_requests::Column::At.lte(unix_ms_to_offset_datetime(to)));
        }
        let count = select.count(&self.db).await?;
        Ok(RequestQueryCount { count })
    }

    pub async fn query_downstream_requests(
        &self,
        query: &DownstreamRequestQuery,
    ) -> Result<Vec<DownstreamRequestQueryRow>, DbErr> {
        let mut select = downstream_requests::Entity::find()
            .order_by_desc(downstream_requests::Column::At)
            .order_by_desc(downstream_requests::Column::TraceId);

        if let Scope::Eq(ref v) = query.trace_id {
            select = select.filter(downstream_requests::Column::TraceId.eq(*v));
        }
        if let Scope::Eq(ref v) = query.user_id {
            select = select.filter(downstream_requests::Column::UserId.eq(*v));
        }
        if let Scope::Eq(ref v) = query.user_key_id {
            select = select.filter(downstream_requests::Column::UserKeyId.eq(*v));
        }
        if let Some(ref contains) = query.request_path_contains {
            select = select.filter(downstream_requests::Column::RequestPath.contains(contains));
        }
        if let Some(from) = query.from_unix_ms {
            select = select
                .filter(downstream_requests::Column::At.gte(unix_ms_to_offset_datetime(from)));
        }
        if let Some(to) = query.to_unix_ms {
            select =
                select.filter(downstream_requests::Column::At.lte(unix_ms_to_offset_datetime(to)));
        }
        select = apply_desc_cursor(
            select,
            query.cursor_at_unix_ms,
            query.cursor_trace_id,
            downstream_requests::Column::At,
            downstream_requests::Column::TraceId,
        );
        if let Some(offset) = query.offset {
            select = select.offset(offset);
        }
        if let Some(limit) = query.limit {
            select = select.limit(limit);
        }

        let rows = select.all(&self.db).await?;
        let include_body = query.include_body.unwrap_or(false);
        Ok(rows
            .into_iter()
            .map(|r| DownstreamRequestQueryRow {
                trace_id: r.trace_id,
                at: r.at,
                internal: r.internal,
                user_id: r.user_id,
                user_key_id: r.user_key_id,
                request_method: r.request_method,
                request_headers_json: r.request_headers_json,
                request_path: r.request_path,
                request_query: r.request_query,
                request_body: if include_body { r.request_body } else { None },
                response_status: r.response_status,
                response_headers_json: r.response_headers_json,
                response_body: if include_body { r.response_body } else { None },
                created_at: r.created_at,
            })
            .collect())
    }

    pub async fn count_downstream_requests(
        &self,
        query: &DownstreamRequestQuery,
    ) -> Result<RequestQueryCount, DbErr> {
        let mut select = downstream_requests::Entity::find();
        if let Scope::Eq(ref v) = query.user_id {
            select = select.filter(downstream_requests::Column::UserId.eq(*v));
        }
        if let Scope::Eq(ref v) = query.user_key_id {
            select = select.filter(downstream_requests::Column::UserKeyId.eq(*v));
        }
        if let Some(from) = query.from_unix_ms {
            select = select
                .filter(downstream_requests::Column::At.gte(unix_ms_to_offset_datetime(from)));
        }
        if let Some(to) = query.to_unix_ms {
            select =
                select.filter(downstream_requests::Column::At.lte(unix_ms_to_offset_datetime(to)));
        }
        let count = select.count(&self.db).await?;
        Ok(RequestQueryCount { count })
    }
}
