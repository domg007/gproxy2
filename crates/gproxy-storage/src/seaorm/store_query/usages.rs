use sea_orm::*;

use crate::query::*;
use crate::seaorm::SeaOrmStorage;
use crate::seaorm::entities::*;
use super::helpers::{apply_desc_cursor, unix_ms_to_offset_datetime};

/// Usage queries — always hit the database at runtime.
/// Usage records are not cached in memory (too large).

impl SeaOrmStorage {
    pub async fn query_usages(&self, query: &UsageQuery) -> Result<Vec<UsageQueryRow>, DbErr> {
        let mut select = usages::Entity::find()
            .order_by_desc(usages::Column::At)
            .order_by_desc(usages::Column::TraceId);

        if let Scope::Eq(ref v) = query.model {
            select = select.filter(usages::Column::Model.eq(v.clone()));
        }
        if let Scope::Eq(ref v) = query.user_id {
            select = select.filter(usages::Column::UserId.eq(*v));
        }
        if let Scope::Eq(ref v) = query.user_key_id {
            select = select.filter(usages::Column::UserKeyId.eq(*v));
        }
        if let Some(from) = query.from_unix_ms {
            select = select.filter(usages::Column::At.gte(unix_ms_to_offset_datetime(from)));
        }
        if let Some(to) = query.to_unix_ms {
            select = select.filter(usages::Column::At.lte(unix_ms_to_offset_datetime(to)));
        }
        select = apply_desc_cursor(
            select,
            query.cursor_at_unix_ms,
            query.cursor_trace_id,
            usages::Column::At,
            usages::Column::TraceId,
        );
        if let Some(offset) = query.offset {
            select = select.offset(offset);
        }
        if let Some(limit) = query.limit {
            select = select.limit(limit);
        }

        let rows = select.all(&self.db).await?;
        Ok(rows.into_iter().map(|r| UsageQueryRow {
            trace_id: r.trace_id,
            downstream_trace_id: r.downstream_trace_id,
            at: r.at,
            provider_id: r.provider_id,
            provider_channel: None, // Would need join; skip for now
            credential_id: r.credential_id,
            user_id: r.user_id,
            user_key_id: r.user_key_id,
            operation: r.operation,
            protocol: r.protocol,
            model: r.model,
            input_tokens: r.input_tokens,
            output_tokens: r.output_tokens,
            cache_read_input_tokens: r.cache_read_input_tokens,
            cache_creation_input_tokens: r.cache_creation_input_tokens,
            cache_creation_input_tokens_5min: r.cache_creation_input_tokens_5min,
            cache_creation_input_tokens_1h: r.cache_creation_input_tokens_1h,
        }).collect())
    }

    pub async fn count_usages(&self, query: &UsageQuery) -> Result<UsageQueryCount, DbErr> {
        let mut select = usages::Entity::find();
        if let Scope::Eq(ref v) = query.model {
            select = select.filter(usages::Column::Model.eq(v.clone()));
        }
        if let Scope::Eq(ref v) = query.user_id {
            select = select.filter(usages::Column::UserId.eq(*v));
        }
        if let Some(from) = query.from_unix_ms {
            select = select.filter(usages::Column::At.gte(unix_ms_to_offset_datetime(from)));
        }
        if let Some(to) = query.to_unix_ms {
            select = select.filter(usages::Column::At.lte(unix_ms_to_offset_datetime(to)));
        }
        let count = select.count(&self.db).await?;
        Ok(UsageQueryCount { count })
    }
}
