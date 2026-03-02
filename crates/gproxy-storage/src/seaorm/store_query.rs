use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, Condition, DbErr, EntityTrait, ExprTrait, FromQueryResult, JoinType, Order,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, RelationTrait,
};
use serde_json::Value;
use time::OffsetDateTime;

use super::SeaOrmStorage;
use super::entities::{
    credential_statuses, credentials, downstream_requests, global_settings, providers,
    upstream_requests, usages, user_keys, users,
};
use crate::query::{
    CredentialQuery, CredentialQueryRow, CredentialStatusQuery, CredentialStatusQueryRow,
    DownstreamRequestQuery, DownstreamRequestQueryRow, GlobalSettingsRow, ProviderQuery,
    ProviderQueryRow, RequestQueryCount, Scope, UpstreamRequestQuery, UpstreamRequestQueryRow,
    UsageQuery, UsageQueryCount, UsageQueryRow, UsageSummary, UserKeyMemoryRow, UserKeyQuery,
    UserKeyQueryRow, UserQuery, UserQueryRow,
};

impl SeaOrmStorage {
    pub async fn get_global_settings(&self) -> Result<Option<GlobalSettingsRow>, DbErr> {
        let row = global_settings::Entity::find()
            .order_by(global_settings::Column::UpdatedAt, Order::Desc)
            .one(self.connection())
            .await?;
        Ok(row.map(|row| GlobalSettingsRow {
            id: row.id,
            host: row.host,
            port: row.port,
            admin_key: row.admin_key,
            hf_token: row.hf_token,
            hf_url: row.hf_url,
            proxy: row.proxy,
            spoof_emulation: row.spoof_emulation,
            dsn: row.dsn,
            data_dir: row.data_dir,
            mask_sensitive_info: row.mask_sensitive_info,
            updated_at: row.updated_at,
        }))
    }

    pub async fn list_providers(
        &self,
        query: &ProviderQuery,
    ) -> Result<Vec<ProviderQueryRow>, DbErr> {
        let mut stmt =
            providers::Entity::find().order_by(providers::Column::UpdatedAt, Order::Desc);
        if let Scope::Eq(channel) = &query.channel {
            stmt = stmt.filter(providers::Column::Channel.eq(channel.as_str()));
        }
        if let Scope::Eq(name) = &query.name {
            stmt = stmt.filter(providers::Column::Name.eq(name.as_str()));
        }
        if let Scope::Eq(enabled) = query.enabled {
            stmt = stmt.filter(providers::Column::Enabled.eq(enabled));
        }
        if let Some(limit) = query.limit
            && limit > 0
        {
            stmt = stmt.limit(limit);
        }
        let rows = stmt.all(self.connection()).await?;
        Ok(rows
            .into_iter()
            .map(|row| ProviderQueryRow {
                id: row.id,
                name: row.name,
                channel: row.channel,
                settings_json: row.settings_json,
                dispatch_json: row.dispatch_json,
                enabled: row.enabled,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
            .collect())
    }

    pub async fn list_credentials(
        &self,
        query: &CredentialQuery,
    ) -> Result<Vec<CredentialQueryRow>, DbErr> {
        let mut stmt =
            credentials::Entity::find().order_by(credentials::Column::UpdatedAt, Order::Desc);
        if let Scope::Eq(provider_id) = query.provider_id {
            stmt = stmt.filter(credentials::Column::ProviderId.eq(provider_id));
        }
        if let Scope::Eq(kind) = &query.kind {
            stmt = stmt.filter(credentials::Column::Kind.eq(kind.as_str()));
        }
        if let Scope::Eq(enabled) = query.enabled {
            stmt = stmt.filter(credentials::Column::Enabled.eq(enabled));
        }
        if let Some(limit) = query.limit
            && limit > 0
        {
            stmt = stmt.limit(limit);
        }
        let rows = stmt.all(self.connection()).await?;
        Ok(rows
            .into_iter()
            .map(|row| CredentialQueryRow {
                id: row.id,
                provider_id: row.provider_id,
                name: row.name,
                kind: row.kind,
                settings_json: row.settings_json,
                secret_json: row.secret_json,
                enabled: row.enabled,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
            .collect())
    }

    pub async fn list_credential_statuses(
        &self,
        query: &CredentialStatusQuery,
    ) -> Result<Vec<CredentialStatusQueryRow>, DbErr> {
        let mut stmt = credential_statuses::Entity::find()
            .order_by(credential_statuses::Column::Id, Order::Desc);
        if let Scope::Eq(id) = query.id {
            stmt = stmt.filter(credential_statuses::Column::Id.eq(id));
        }
        if let Scope::Eq(credential_id) = query.credential_id {
            stmt = stmt.filter(credential_statuses::Column::CredentialId.eq(credential_id));
        }
        if let Scope::Eq(channel) = &query.channel {
            stmt = stmt.filter(credential_statuses::Column::Channel.eq(channel.as_str()));
        }
        if let Scope::Eq(health_kind) = &query.health_kind {
            stmt = stmt.filter(credential_statuses::Column::HealthKind.eq(health_kind.as_str()));
        }
        if let Some(limit) = query.limit
            && limit > 0
        {
            stmt = stmt.limit(limit);
        }
        let rows = stmt.all(self.connection()).await?;
        Ok(rows
            .into_iter()
            .map(|row| CredentialStatusQueryRow {
                id: row.id,
                credential_id: row.credential_id,
                channel: row.channel,
                health_kind: row.health_kind,
                health_json: row.health_json,
                checked_at: row.checked_at,
                last_error: row.last_error,
                updated_at: row.updated_at,
            })
            .collect())
    }

    pub async fn list_users(&self, query: &UserQuery) -> Result<Vec<UserQueryRow>, DbErr> {
        let mut stmt = users::Entity::find().order_by(users::Column::UpdatedAt, Order::Desc);
        if let Scope::Eq(id) = query.id {
            stmt = stmt.filter(users::Column::Id.eq(id));
        }
        if let Scope::Eq(name) = &query.name {
            stmt = stmt.filter(users::Column::Name.eq(name.as_str()));
        }
        let rows = stmt.all(self.connection()).await?;
        Ok(rows
            .into_iter()
            .map(|row| UserQueryRow {
                id: row.id,
                name: row.name,
                password: row.password.unwrap_or_default(),
                enabled: row.enabled,
            })
            .collect())
    }

    pub async fn list_user_keys(
        &self,
        query: &UserKeyQuery,
    ) -> Result<Vec<UserKeyQueryRow>, DbErr> {
        let mut stmt =
            user_keys::Entity::find().order_by(user_keys::Column::UpdatedAt, Order::Desc);
        if let Scope::Eq(id) = query.id {
            stmt = stmt.filter(user_keys::Column::Id.eq(id));
        }
        if let Scope::Eq(user_id) = query.user_id {
            stmt = stmt.filter(user_keys::Column::UserId.eq(user_id));
        }
        if let Scope::Eq(api_key) = &query.api_key {
            stmt = stmt.filter(user_keys::Column::ApiKey.eq(api_key.as_str()));
        }
        let rows = stmt.all(self.connection()).await?;
        Ok(rows
            .into_iter()
            .map(|row| UserKeyQueryRow {
                id: row.id,
                user_id: row.user_id,
                api_key: row.api_key,
            })
            .collect())
    }

    pub async fn list_user_keys_for_memory(
        &self,
        query: &UserKeyQuery,
    ) -> Result<Vec<UserKeyMemoryRow>, DbErr> {
        let mut stmt =
            user_keys::Entity::find().order_by(user_keys::Column::UpdatedAt, Order::Desc);
        if let Scope::Eq(id) = query.id {
            stmt = stmt.filter(user_keys::Column::Id.eq(id));
        }
        if let Scope::Eq(user_id) = query.user_id {
            stmt = stmt.filter(user_keys::Column::UserId.eq(user_id));
        }
        if let Scope::Eq(api_key) = &query.api_key {
            stmt = stmt.filter(user_keys::Column::ApiKey.eq(api_key.as_str()));
        }
        let rows = stmt.all(self.connection()).await?;
        Ok(rows
            .into_iter()
            .map(|row| UserKeyMemoryRow {
                id: row.id,
                user_id: row.user_id,
                api_key: row.api_key,
                enabled: row.enabled,
            })
            .collect())
    }

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

    pub async fn query_usages(&self, query: &UsageQuery) -> Result<Vec<UsageQueryRow>, DbErr> {
        let mut stmt = usages::Entity::find()
            .join(JoinType::LeftJoin, usages::Relation::Providers.def())
            .select_only()
            .column(usages::Column::TraceId)
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

#[derive(Debug, Clone, FromQueryResult)]
struct UsageQueryRowModel {
    pub trace_id: i64,
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

fn unix_ms_to_offset_datetime(unix_ms: i64) -> Result<OffsetDateTime, time::error::ComponentRange> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(unix_ms) * 1_000_000)
}
