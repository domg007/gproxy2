use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, Condition, DbErr, EntityTrait, ExprTrait, FromQueryResult, JoinType, Order,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait,
};
use time::OffsetDateTime;

use super::super::SeaOrmStorage;
use super::super::entities::{providers, usages};
use super::helpers::unix_ms_to_offset_datetime;
use crate::query::{Scope, UsageQuery, UsageQueryCount, UsageQueryRow, UsageSummary};

impl SeaOrmStorage {
    pub async fn query_usages(&self, query: &UsageQuery) -> Result<Vec<UsageQueryRow>, DbErr> {
        let mut stmt = usages::Entity::find()
            .join(JoinType::LeftJoin, usages::Relation::Providers.def())
            .select_only()
            .column(usages::Column::TraceId)
            .column(usages::Column::DownstreamTraceId)
            .column(usages::Column::At)
            .column(usages::Column::ProviderId)
            .column_as(providers::Column::Channel, "provider_channel")
            .column(usages::Column::CredentialId)
            .column(usages::Column::UserId)
            .column(usages::Column::UserKeyId)
            .column(usages::Column::Operation)
            .column(usages::Column::Protocol)
            .column(usages::Column::Model)
            .column(usages::Column::InputTokens)
            .column(usages::Column::OutputTokens)
            .column(usages::Column::CacheReadInputTokens)
            .column(usages::Column::CacheCreationInputTokens)
            .column(usages::Column::CacheCreationInputTokens5min)
            .column(usages::Column::CacheCreationInputTokens1h)
            .order_by(usages::Column::At, Order::Desc)
            .order_by(usages::Column::TraceId, Order::Desc);

        stmt = apply_usage_filters(stmt, query);
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
            .into_model::<UsageQueryRowModel>()
            .all(self.connection())
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn summarize_usages(&self, query: &UsageQuery) -> Result<UsageSummary, DbErr> {
        let mut stmt = usages::Entity::find()
            .join(JoinType::LeftJoin, usages::Relation::Providers.def())
            .select_only()
            .column_as(Expr::col(usages::Column::TraceId).count(), "count")
            .column_as(Expr::col(usages::Column::InputTokens).sum(), "input_tokens")
            .column_as(
                Expr::col(usages::Column::OutputTokens).sum(),
                "output_tokens",
            )
            .column_as(
                Expr::col(usages::Column::CacheReadInputTokens).sum(),
                "cache_read_input_tokens",
            )
            .column_as(
                Expr::col(usages::Column::CacheCreationInputTokens).sum(),
                "cache_creation_input_tokens",
            )
            .column_as(
                Expr::col(usages::Column::CacheCreationInputTokens5min).sum(),
                "cache_creation_input_tokens_5min",
            )
            .column_as(
                Expr::col(usages::Column::CacheCreationInputTokens1h).sum(),
                "cache_creation_input_tokens_1h",
            );

        stmt = apply_usage_filters(stmt, query);
        let Some(row) = stmt
            .into_model::<UsageSummaryModel>()
            .one(self.connection())
            .await?
        else {
            return Ok(UsageSummary::default());
        };

        let count = u64::try_from(row.count).unwrap_or(0);
        Ok(UsageSummary {
            count,
            input_tokens: row.input_tokens.unwrap_or(0),
            output_tokens: row.output_tokens.unwrap_or(0),
            cache_read_input_tokens: row.cache_read_input_tokens.unwrap_or(0),
            cache_creation_input_tokens: row.cache_creation_input_tokens.unwrap_or(0),
            cache_creation_input_tokens_5min: row.cache_creation_input_tokens_5min.unwrap_or(0),
            cache_creation_input_tokens_1h: row.cache_creation_input_tokens_1h.unwrap_or(0),
        })
    }

    pub async fn count_usages(&self, query: &UsageQuery) -> Result<UsageQueryCount, DbErr> {
        let mut stmt =
            usages::Entity::find().join(JoinType::LeftJoin, usages::Relation::Providers.def());
        stmt = apply_usage_filters(stmt, query);
        let count = stmt.count(self.connection()).await?;
        Ok(UsageQueryCount { count })
    }
}

fn apply_usage_filters<S>(stmt: S, query: &UsageQuery) -> S
where
    S: QueryFilter,
{
    let mut condition = Condition::all();

    if let Scope::Eq(channel) = &query.channel {
        condition = condition.add(providers::Column::Channel.eq(channel.as_str()));
    }
    if let Scope::Eq(model) = &query.model {
        condition = condition.add(usages::Column::Model.eq(model.as_str()));
    }
    if let Scope::Eq(user_id) = &query.user_id {
        condition = condition.add(usages::Column::UserId.eq(*user_id));
    }
    if let Scope::Eq(user_key_id) = &query.user_key_id {
        condition = condition.add(usages::Column::UserKeyId.eq(*user_key_id));
    }
    if let Some(from_unix_ms) = query.from_unix_ms
        && let Ok(from) = unix_ms_to_offset_datetime(from_unix_ms)
    {
        condition = condition.add(usages::Column::At.gte(from));
    }
    if let Some(to_unix_ms) = query.to_unix_ms
        && let Ok(to) = unix_ms_to_offset_datetime(to_unix_ms)
    {
        condition = condition.add(usages::Column::At.lt(to));
    }

    stmt.filter(condition)
}

#[derive(Debug, Clone, FromQueryResult)]
struct UsageQueryRowModel {
    pub trace_id: i64,
    pub downstream_trace_id: Option<i64>,
    pub at: OffsetDateTime,
    pub provider_id: Option<i64>,
    pub provider_channel: Option<String>,
    pub credential_id: Option<i64>,
    pub user_id: Option<i64>,
    pub user_key_id: Option<i64>,
    pub operation: String,
    pub protocol: String,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_input_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
    pub cache_creation_input_tokens_5min: Option<i64>,
    pub cache_creation_input_tokens_1h: Option<i64>,
}

impl From<UsageQueryRowModel> for UsageQueryRow {
    fn from(value: UsageQueryRowModel) -> Self {
        Self {
            trace_id: value.trace_id,
            downstream_trace_id: value.downstream_trace_id,
            at: value.at,
            provider_id: value.provider_id,
            provider_channel: value.provider_channel,
            credential_id: value.credential_id,
            user_id: value.user_id,
            user_key_id: value.user_key_id,
            operation: value.operation,
            protocol: value.protocol,
            model: value.model,
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            cache_read_input_tokens: value.cache_read_input_tokens,
            cache_creation_input_tokens: value.cache_creation_input_tokens,
            cache_creation_input_tokens_5min: value.cache_creation_input_tokens_5min,
            cache_creation_input_tokens_1h: value.cache_creation_input_tokens_1h,
        }
    }
}

#[derive(Debug, Clone, FromQueryResult)]
struct UsageSummaryModel {
    pub count: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_input_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
    pub cache_creation_input_tokens_5min: Option<i64>,
    pub cache_creation_input_tokens_1h: Option<i64>,
}
