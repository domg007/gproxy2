use std::collections::HashMap;

use sea_orm::{
    ActiveModelTrait, ActiveValue, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection,
    EntityTrait, QueryOrder, Schema,
};
use sea_orm::{ColumnTrait, QueryFilter};
use time::OffsetDateTime;

use gproxy_common::GlobalConfig;
use gproxy_provider_core::Event;

use crate::entities;
use crate::snapshot::{
    CredentialRow, GlobalConfigRow, ProviderRow, StorageSnapshot, UserKeyRow, UserRow,
};
use crate::storage::{Storage, StorageError, StorageResult, UsageAggregate, UsageAggregateFilter};

#[derive(Clone)]
pub struct SeaOrmStorage {
    db: DatabaseConnection,
}

impl SeaOrmStorage {
    pub async fn connect(dsn: &str) -> StorageResult<Self> {
        let db = Database::connect(dsn).await?;
        // Ensure sqlite enforces foreign keys (required for cascade + integrity).
        if db.get_database_backend() == DatabaseBackend::Sqlite {
            db.execute_unprepared("PRAGMA foreign_keys = ON").await?;
        }
        Ok(Self { db })
    }

    pub fn connection(&self) -> &DatabaseConnection {
        &self.db
    }

    pub async fn provider_names(&self) -> StorageResult<Vec<String>> {
        let rows = entities::Providers::find().all(&self.db).await?;
        Ok(rows.into_iter().map(|m| m.name).collect())
    }

    async fn backfill_usage_models(&self) -> StorageResult<()> {
        use entities::upstream_requests::Column as UpstreamRequestColumn;
        use entities::upstream_usages::Column as UpstreamUsageColumn;

        let usage_rows = entities::UpstreamUsages::find()
            .filter(UpstreamUsageColumn::Model.is_null())
            .filter(
                UpstreamUsageColumn::Operation
                    .is_in(vec!["GenerateContent", "StreamGenerateContent"]),
            )
            .all(&self.db)
            .await?;
        if usage_rows.is_empty() {
            return Ok(());
        }

        let request_ids: Vec<i64> = usage_rows
            .iter()
            .map(|row| row.upstream_request_id)
            .collect();
        let request_rows = entities::UpstreamRequests::find()
            .filter(UpstreamRequestColumn::Id.is_in(request_ids))
            .all(&self.db)
            .await?;
        let model_by_request_id: HashMap<i64, Option<String>> = request_rows
            .into_iter()
            .map(|row| {
                (
                    row.id,
                    extract_model_for_usage(&row.request_path, row.request_body.as_deref()),
                )
            })
            .collect();

        for row in usage_rows {
            let Some(model) = model_by_request_id
                .get(&row.upstream_request_id)
                .and_then(|m| m.clone())
            else {
                continue;
            };
            let mut active: entities::upstream_usages::ActiveModel = row.into();
            active.model = ActiveValue::Set(Some(model));
            active.update(&self.db).await?;
        }

        Ok(())
    }

    async fn ensure_usage_model_column(&self) -> StorageResult<()> {
        if self.db.get_database_backend() != DatabaseBackend::Sqlite {
            return Ok(());
        }

        let res = self
            .db
            .execute_unprepared("ALTER TABLE upstream_usages ADD COLUMN model TEXT")
            .await;
        if let Err(err) = res {
            let msg = err.to_string().to_ascii_lowercase();
            if !msg.contains("duplicate column name") {
                return Err(err.into());
            }
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl Storage for SeaOrmStorage {
    async fn sync(&self) -> StorageResult<()> {
        Schema::new(self.db.get_database_backend())
            .builder()
            .register(entities::GlobalConfig)
            .register(entities::Providers)
            .register(entities::Credentials)
            .register(entities::Users)
            .register(entities::UserKeys)
            .register(entities::DownstreamRequests)
            .register(entities::UpstreamRequests)
            .register(entities::UpstreamUsages)
            .register(entities::InternalEvents)
            .sync(&self.db)
            .await?;
        self.ensure_usage_model_column().await?;
        self.backfill_usage_models().await?;
        Ok(())
    }

    async fn load_global_config(&self) -> StorageResult<Option<GlobalConfigRow>> {
        use entities::global_config::Column;
        let row = entities::GlobalConfig::find()
            .order_by_asc(Column::Id)
            .one(&self.db)
            .await?;
        Ok(row.map(|m| GlobalConfigRow {
            id: m.id,
            config: GlobalConfig {
                host: m.host,
                port: u16::try_from(m.port).unwrap_or(8787),
                admin_key: m.admin_key,
                proxy: m.proxy,
                dsn: m.dsn,
                event_redact_sensitive: m.event_redact_sensitive.unwrap_or(true),
            },
            updated_at: m.updated_at,
        }))
    }

    async fn upsert_global_config(&self, config: &GlobalConfig) -> StorageResult<()> {
        use entities::global_config::ActiveModel as GlobalActive;

        let now = OffsetDateTime::now_utc();
        let id = 1_i64;

        let existing = entities::GlobalConfig::find_by_id(id).one(&self.db).await?;

        match existing {
            Some(model) => {
                // Convert Model -> ActiveModel for update.
                let mut active: GlobalActive = model.into();
                active.host = ActiveValue::Set(config.host.clone());
                active.port = ActiveValue::Set(i32::from(config.port));
                active.admin_key = ActiveValue::Set(config.admin_key.clone());
                active.proxy = ActiveValue::Set(config.proxy.clone());
                active.dsn = ActiveValue::Set(config.dsn.clone());
                active.event_redact_sensitive =
                    ActiveValue::Set(Some(config.event_redact_sensitive));
                active.updated_at = ActiveValue::Set(now);
                active.update(&self.db).await?;
            }
            None => {
                let active = GlobalActive {
                    id: ActiveValue::Set(id),
                    host: ActiveValue::Set(config.host.clone()),
                    port: ActiveValue::Set(i32::from(config.port)),
                    admin_key: ActiveValue::Set(config.admin_key.clone()),
                    proxy: ActiveValue::Set(config.proxy.clone()),
                    dsn: ActiveValue::Set(config.dsn.clone()),
                    event_redact_sensitive: ActiveValue::Set(Some(config.event_redact_sensitive)),
                    updated_at: ActiveValue::Set(now),
                };
                entities::GlobalConfig::insert(active)
                    .exec(&self.db)
                    .await?;
            }
        }

        Ok(())
    }

    async fn load_snapshot(&self) -> StorageResult<StorageSnapshot> {
        let global_config = self.load_global_config().await?;

        let providers = entities::Providers::find().all(&self.db).await?;
        let providers = providers
            .into_iter()
            .map(|m| ProviderRow {
                id: m.id,
                name: m.name,
                config_json: m.config_json,
                enabled: m.enabled,
                updated_at: m.updated_at,
            })
            .collect();

        let credentials = entities::Credentials::find().all(&self.db).await?;
        let credentials = credentials
            .into_iter()
            .map(|m| CredentialRow {
                id: m.id,
                provider_id: m.provider_id,
                name: m.name,
                settings_json: m.settings.unwrap_or_else(|| serde_json::json!({})),
                secret_json: m.secret,
                enabled: m.enabled,
                created_at: m.created_at,
                updated_at: m.updated_at,
            })
            .collect();

        let users = entities::Users::find().all(&self.db).await?;
        let users = users
            .into_iter()
            .map(|m| UserRow {
                id: m.id,
                name: m.name,
                enabled: m.enabled,
                created_at: m.created_at,
                updated_at: m.updated_at,
            })
            .collect();

        let user_keys = entities::UserKeys::find().all(&self.db).await?;
        let user_keys = user_keys
            .into_iter()
            .map(|m| UserKeyRow {
                id: m.id,
                user_id: m.user_id,
                api_key: m.api_key,
                label: m.label,
                enabled: m.enabled,
                created_at: m.created_at,
                updated_at: m.updated_at,
            })
            .collect();

        Ok(StorageSnapshot {
            global_config,
            providers,
            credentials,
            users,
            user_keys,
        })
    }

    async fn upsert_provider(
        &self,
        name: &str,
        config_json: &serde_json::Value,
        enabled: bool,
    ) -> StorageResult<i64> {
        use entities::providers::{ActiveModel as ProviderActive, Column};

        let now = OffsetDateTime::now_utc();
        let existing = entities::Providers::find()
            .filter(Column::Name.eq(name))
            .one(&self.db)
            .await?;

        let id = match existing {
            Some(model) => {
                let mut active: ProviderActive = model.into();
                active.config_json = ActiveValue::Set(config_json.clone());
                active.enabled = ActiveValue::Set(enabled);
                active.updated_at = ActiveValue::Set(now);
                let updated = active.update(&self.db).await?;
                updated.id
            }
            None => {
                let active = ProviderActive {
                    id: ActiveValue::NotSet,
                    name: ActiveValue::Set(name.to_string()),
                    config_json: ActiveValue::Set(config_json.clone()),
                    enabled: ActiveValue::Set(enabled),
                    updated_at: ActiveValue::Set(now),
                };
                let inserted = entities::Providers::insert(active).exec(&self.db).await?;
                inserted.last_insert_id
            }
        };
        Ok(id)
    }

    async fn delete_provider(&self, name: &str) -> StorageResult<()> {
        use entities::providers::Column as ProviderColumn;

        let provider = entities::Providers::find()
            .filter(ProviderColumn::Name.eq(name))
            .one(&self.db)
            .await?;
        let Some(provider) = provider else {
            return Ok(());
        };

        // Rely on DB-level ON DELETE CASCADE for credentials.
        entities::Providers::delete_by_id(provider.id)
            .exec(&self.db)
            .await?;
        Ok(())
    }

    async fn insert_credential(
        &self,
        provider_name: &str,
        name: Option<&str>,
        settings_json: &serde_json::Value,
        secret_json: &serde_json::Value,
        enabled: bool,
    ) -> StorageResult<i64> {
        use entities::credentials::ActiveModel as CredentialActive;
        use entities::providers::Column as ProviderColumn;

        let provider = entities::Providers::find()
            .filter(ProviderColumn::Name.eq(provider_name))
            .one(&self.db)
            .await?;
        let Some(provider) = provider else {
            return Err(StorageError::Db(sea_orm::DbErr::RecordNotFound(format!(
                "provider not found: {provider_name}"
            ))));
        };

        let now = OffsetDateTime::now_utc();
        let active = CredentialActive {
            id: ActiveValue::NotSet,
            provider_id: ActiveValue::Set(provider.id),
            name: ActiveValue::Set(name.map(|s| s.to_string())),
            settings: ActiveValue::Set(Some(settings_json.clone())),
            secret: ActiveValue::Set(secret_json.clone()),
            enabled: ActiveValue::Set(enabled),
            created_at: ActiveValue::Set(now),
            updated_at: ActiveValue::Set(now),
        };
        let inserted = entities::Credentials::insert(active).exec(&self.db).await?;
        Ok(inserted.last_insert_id)
    }

    async fn update_credential(
        &self,
        credential_id: i64,
        name: Option<&str>,
        settings_json: &serde_json::Value,
        secret_json: &serde_json::Value,
    ) -> StorageResult<()> {
        use entities::credentials::ActiveModel as CredentialActive;

        let existing = entities::Credentials::find_by_id(credential_id)
            .one(&self.db)
            .await?;
        let Some(model) = existing else {
            return Ok(());
        };

        let now = OffsetDateTime::now_utc();
        let mut active: CredentialActive = model.into();
        active.name = ActiveValue::Set(name.map(|s| s.to_string()));
        active.settings = ActiveValue::Set(Some(settings_json.clone()));
        active.secret = ActiveValue::Set(secret_json.clone());
        active.updated_at = ActiveValue::Set(now);
        active.update(&self.db).await?;
        Ok(())
    }

    async fn set_credential_enabled(&self, credential_id: i64, enabled: bool) -> StorageResult<()> {
        use entities::credentials::ActiveModel as CredentialActive;

        let now = OffsetDateTime::now_utc();
        let existing = entities::Credentials::find_by_id(credential_id)
            .one(&self.db)
            .await?;
        let Some(model) = existing else {
            return Ok(());
        };
        let mut active: CredentialActive = model.into();
        active.enabled = ActiveValue::Set(enabled);
        active.updated_at = ActiveValue::Set(now);
        active.update(&self.db).await?;
        Ok(())
    }

    async fn delete_credential(&self, credential_id: i64) -> StorageResult<()> {
        entities::Credentials::delete_by_id(credential_id)
            .exec(&self.db)
            .await?;
        Ok(())
    }

    async fn upsert_user_by_id(
        &self,
        user_id: i64,
        name: &str,
        enabled: bool,
    ) -> StorageResult<()> {
        use entities::users::ActiveModel as UserActive;

        let now = OffsetDateTime::now_utc();
        let existing = entities::Users::find_by_id(user_id).one(&self.db).await?;

        match existing {
            Some(model) => {
                let mut active: UserActive = model.into();
                active.name = ActiveValue::Set(name.to_string());
                active.enabled = ActiveValue::Set(enabled);
                active.updated_at = ActiveValue::Set(now);
                active.update(&self.db).await?;
            }
            None => {
                let active = UserActive {
                    id: ActiveValue::Set(user_id),
                    name: ActiveValue::Set(name.to_string()),
                    enabled: ActiveValue::Set(enabled),
                    created_at: ActiveValue::Set(now),
                    updated_at: ActiveValue::Set(now),
                };
                entities::Users::insert(active).exec(&self.db).await?;
            }
        }
        Ok(())
    }

    async fn set_user_enabled(&self, user_id: i64, enabled: bool) -> StorageResult<()> {
        use entities::users::ActiveModel as UserActive;

        let now = OffsetDateTime::now_utc();
        let existing = entities::Users::find_by_id(user_id).one(&self.db).await?;
        let Some(model) = existing else {
            return Ok(());
        };
        let mut active: UserActive = model.into();
        active.enabled = ActiveValue::Set(enabled);
        active.updated_at = ActiveValue::Set(now);
        active.update(&self.db).await?;
        Ok(())
    }

    async fn delete_user(&self, user_id: i64) -> StorageResult<()> {
        // Rely on DB-level ON DELETE CASCADE for user_keys.
        entities::Users::delete_by_id(user_id)
            .exec(&self.db)
            .await?;
        Ok(())
    }

    async fn insert_user_key(
        &self,
        user_id: i64,
        api_key: &str,
        label: Option<&str>,
        enabled: bool,
    ) -> StorageResult<i64> {
        use entities::user_keys::ActiveModel as UserKeyActive;

        let now = OffsetDateTime::now_utc();
        let active = UserKeyActive {
            id: ActiveValue::NotSet,
            user_id: ActiveValue::Set(user_id),
            api_key: ActiveValue::Set(api_key.to_string()),
            label: ActiveValue::Set(label.map(|s| s.to_string())),
            enabled: ActiveValue::Set(enabled),
            created_at: ActiveValue::Set(now),
            updated_at: ActiveValue::Set(now),
        };
        let inserted = entities::UserKeys::insert(active).exec(&self.db).await?;
        Ok(inserted.last_insert_id)
    }

    async fn set_user_key_enabled(&self, user_key_id: i64, enabled: bool) -> StorageResult<()> {
        use entities::user_keys::ActiveModel as UserKeyActive;

        let now = OffsetDateTime::now_utc();
        let existing = entities::UserKeys::find_by_id(user_key_id)
            .one(&self.db)
            .await?;
        let Some(model) = existing else {
            return Ok(());
        };
        let mut active: UserKeyActive = model.into();
        active.enabled = ActiveValue::Set(enabled);
        active.updated_at = ActiveValue::Set(now);
        active.update(&self.db).await?;
        Ok(())
    }

    async fn update_user_key_label(
        &self,
        user_key_id: i64,
        label: Option<&str>,
    ) -> StorageResult<()> {
        use entities::user_keys::ActiveModel as UserKeyActive;

        let existing = entities::UserKeys::find_by_id(user_key_id)
            .one(&self.db)
            .await?;
        let Some(model) = existing else {
            return Ok(());
        };
        let now = OffsetDateTime::now_utc();
        let mut active: UserKeyActive = model.into();
        active.label = ActiveValue::Set(label.map(|s| s.to_string()));
        active.updated_at = ActiveValue::Set(now);
        active.update(&self.db).await?;
        Ok(())
    }

    async fn delete_user_key(&self, user_key_id: i64) -> StorageResult<()> {
        entities::UserKeys::delete_by_id(user_key_id)
            .exec(&self.db)
            .await?;
        Ok(())
    }

    async fn append_event(&self, event: &Event) -> StorageResult<()> {
        let now = OffsetDateTime::now_utc();
        match event {
            Event::Downstream(ev) => {
                use entities::downstream_requests::ActiveModel as DownstreamActive;
                let active = DownstreamActive {
                    id: ActiveValue::NotSet,
                    trace_id: ActiveValue::Set(ev.trace_id.clone()),
                    at: ActiveValue::Set(system_time_to_offset(ev.at)),
                    user_id: ActiveValue::Set(ev.user_id),
                    user_key_id: ActiveValue::Set(ev.user_key_id),
                    request_method: ActiveValue::Set(ev.request_method.clone()),
                    request_headers_json: ActiveValue::Set(serde_json::to_value(
                        &ev.request_headers,
                    )?),
                    request_path: ActiveValue::Set(ev.request_path.clone()),
                    request_query: ActiveValue::Set(ev.request_query.clone()),
                    request_body: ActiveValue::Set(ev.request_body.clone()),
                    response_status: ActiveValue::Set(ev.response_status.map(i32::from)),
                    response_headers_json: ActiveValue::Set(serde_json::to_value(
                        &ev.response_headers,
                    )?),
                    response_body: ActiveValue::Set(ev.response_body.clone()),
                    created_at: ActiveValue::Set(now),
                };
                entities::DownstreamRequests::insert(active)
                    .exec(&self.db)
                    .await?;
            }
            Event::Upstream(ev) => {
                use entities::upstream_requests::ActiveModel as UpstreamActive;
                use entities::upstream_usages::ActiveModel as UpstreamUsageActive;
                let active = UpstreamActive {
                    id: ActiveValue::NotSet,
                    trace_id: ActiveValue::Set(ev.trace_id.clone()),
                    at: ActiveValue::Set(system_time_to_offset(ev.at)),
                    user_id: ActiveValue::Set(ev.user_id),
                    user_key_id: ActiveValue::Set(ev.user_key_id),
                    provider: ActiveValue::Set(ev.provider.clone()),
                    credential_id: ActiveValue::Set(ev.credential_id),
                    internal: ActiveValue::Set(ev.internal),
                    attempt_no: ActiveValue::Set(i32::try_from(ev.attempt_no).unwrap_or(i32::MAX)),
                    operation: ActiveValue::Set(ev.operation.clone()),
                    request_method: ActiveValue::Set(ev.request_method.clone()),
                    request_headers_json: ActiveValue::Set(serde_json::to_value(
                        &ev.request_headers,
                    )?),
                    request_path: ActiveValue::Set(ev.request_path.clone()),
                    request_query: ActiveValue::Set(ev.request_query.clone()),
                    request_body: ActiveValue::Set(ev.request_body.clone()),
                    response_status: ActiveValue::Set(ev.response_status.map(i32::from)),
                    response_headers_json: ActiveValue::Set(serde_json::to_value(
                        &ev.response_headers,
                    )?),
                    response_body: ActiveValue::Set(ev.response_body.clone()),
                    error_kind: ActiveValue::Set(ev.error_kind.clone()),
                    error_message: ActiveValue::Set(ev.error_message.clone()),
                    transport_kind: ActiveValue::Set(ev.transport_kind.map(|k| format!("{k:?}"))),
                    created_at: ActiveValue::Set(now),
                };
                let inserted = entities::UpstreamRequests::insert(active)
                    .exec(&self.db)
                    .await?;
                if let Some(usage) = &ev.usage {
                    let model = match ev.operation.as_str() {
                        "GenerateContent" | "StreamGenerateContent" => {
                            extract_model_for_usage(&ev.request_path, ev.request_body.as_deref())
                        }
                        _ => None,
                    };
                    let usage_active = UpstreamUsageActive {
                        id: ActiveValue::NotSet,
                        upstream_request_id: ActiveValue::Set(inserted.last_insert_id),
                        trace_id: ActiveValue::Set(ev.trace_id.clone()),
                        at: ActiveValue::Set(system_time_to_offset(ev.at)),
                        user_id: ActiveValue::Set(ev.user_id),
                        user_key_id: ActiveValue::Set(ev.user_key_id),
                        provider: ActiveValue::Set(ev.provider.clone()),
                        credential_id: ActiveValue::Set(ev.credential_id),
                        internal: ActiveValue::Set(ev.internal),
                        attempt_no: ActiveValue::Set(
                            i32::try_from(ev.attempt_no).unwrap_or(i32::MAX),
                        ),
                        operation: ActiveValue::Set(ev.operation.clone()),
                        model: ActiveValue::Set(model),
                        input_tokens: ActiveValue::Set(usage.input_tokens.map(i64::from)),
                        output_tokens: ActiveValue::Set(usage.output_tokens.map(i64::from)),
                        cache_read_input_tokens: ActiveValue::Set(
                            usage.cache_read_input_tokens.map(i64::from),
                        ),
                        cache_creation_input_tokens: ActiveValue::Set(
                            usage.cache_creation_input_tokens.map(i64::from),
                        ),
                        created_at: ActiveValue::Set(now),
                    };
                    entities::UpstreamUsages::insert(usage_active)
                        .exec(&self.db)
                        .await?;
                }
            }
            Event::Operational(ev) => {
                use entities::internal_events::ActiveModel as InternalActive;
                let active = InternalActive {
                    id: ActiveValue::NotSet,
                    event_type: ActiveValue::Set(match ev {
                        gproxy_provider_core::OperationalEvent::UnavailableStart(_) => {
                            "unavailable_start".to_string()
                        }
                        gproxy_provider_core::OperationalEvent::UnavailableEnd(_) => {
                            "unavailable_end".to_string()
                        }
                        gproxy_provider_core::OperationalEvent::ModelUnavailableStart(_) => {
                            "model_unavailable_start".to_string()
                        }
                        gproxy_provider_core::OperationalEvent::ModelUnavailableEnd(_) => {
                            "model_unavailable_end".to_string()
                        }
                    }),
                    payload_json: ActiveValue::Set(serde_json::to_value(ev)?),
                    at: ActiveValue::Set(extract_operational_at(ev)),
                    created_at: ActiveValue::Set(now),
                };
                entities::InternalEvents::insert(active)
                    .exec(&self.db)
                    .await?;
            }
        }
        Ok(())
    }

    async fn aggregate_usage_tokens(
        &self,
        filter: UsageAggregateFilter,
    ) -> StorageResult<UsageAggregate> {
        use entities::upstream_usages::Column as UpstreamUsageColumn;

        let mut usage_query = entities::UpstreamUsages::find()
            .filter(UpstreamUsageColumn::At.gte(filter.from))
            .filter(UpstreamUsageColumn::At.lte(filter.to));

        if let Some(provider) = filter.provider.as_deref() {
            usage_query = usage_query.filter(UpstreamUsageColumn::Provider.eq(provider));
        }
        if let Some(credential_id) = filter.credential_id {
            usage_query = usage_query.filter(UpstreamUsageColumn::CredentialId.eq(credential_id));
        }
        if let Some(model) = filter.model.as_deref() {
            usage_query = usage_query.filter(UpstreamUsageColumn::Model.eq(model));
        }
        if let Some(model_contains) = filter.model_contains.as_deref() {
            usage_query = usage_query.filter(UpstreamUsageColumn::Model.contains(model_contains));
        }

        let usage_rows = usage_query.all(&self.db).await?;
        if usage_rows.is_empty() {
            return Ok(UsageAggregate::default());
        }

        let mut out = UsageAggregate {
            matched_rows: i64::try_from(usage_rows.len()).unwrap_or(i64::MAX),
            ..UsageAggregate::default()
        };

        for row in usage_rows {
            out.input_tokens += row.input_tokens.unwrap_or(0);
            out.output_tokens += row.output_tokens.unwrap_or(0);
            out.cache_read_input_tokens += row.cache_read_input_tokens.unwrap_or(0);
            out.cache_creation_input_tokens += row.cache_creation_input_tokens.unwrap_or(0);
        }
        out.total_tokens = out.input_tokens
            + out.output_tokens
            + out.cache_read_input_tokens
            + out.cache_creation_input_tokens;

        Ok(out)
    }
}

fn system_time_to_offset(at: std::time::SystemTime) -> OffsetDateTime {
    match at.duration_since(std::time::UNIX_EPOCH) {
        Ok(dur) => OffsetDateTime::from_unix_timestamp_nanos(dur.as_nanos() as i128)
            .unwrap_or_else(|_| OffsetDateTime::now_utc()),
        Err(_) => OffsetDateTime::now_utc(),
    }
}

fn extract_operational_at(ev: &gproxy_provider_core::OperationalEvent) -> OffsetDateTime {
    match ev {
        gproxy_provider_core::OperationalEvent::UnavailableStart(v) => system_time_to_offset(v.at),
        gproxy_provider_core::OperationalEvent::UnavailableEnd(v) => system_time_to_offset(v.at),
        gproxy_provider_core::OperationalEvent::ModelUnavailableStart(v) => {
            system_time_to_offset(v.at)
        }
        gproxy_provider_core::OperationalEvent::ModelUnavailableEnd(v) => {
            system_time_to_offset(v.at)
        }
    }
}

fn extract_model_for_usage(request_path: &str, request_body: Option<&[u8]>) -> Option<String> {
    if let Some(body) = request_body
        && let Ok(json) = serde_json::from_slice::<serde_json::Value>(body)
        && let Some(model) = json.get("model").and_then(|v| v.as_str())
    {
        let model = model.trim();
        if !model.is_empty() {
            return Some(model.to_string());
        }
    }

    if let Some(idx) = request_path.find("/models/") {
        let rest = &request_path[(idx + "/models/".len())..];
        if let Some(model) = normalize_model_candidate(rest, true) {
            return Some(model);
        }
    }

    if let Some(rest) = request_path.strip_prefix("/v1beta/")
        && let Some(model) = normalize_model_candidate(rest, false)
    {
        return Some(model);
    }

    if let Some(rest) = request_path.strip_prefix("/v1/")
        && let Some(model) = normalize_model_candidate(rest, false)
    {
        return Some(model);
    }

    None
}

fn normalize_model_candidate(raw: &str, allow_no_action_suffix: bool) -> Option<String> {
    let mut s = raw.trim();
    s = s.trim_start_matches('/');
    let s = s
        .split('?')
        .next()
        .unwrap_or(s)
        .split('#')
        .next()
        .unwrap_or(s)
        .trim_end_matches('/');

    let s = if let Some((model, action)) = s.rsplit_once(':') {
        if action.trim().is_empty() {
            return None;
        }
        model
    } else if allow_no_action_suffix {
        s
    } else {
        return None;
    };

    let mut s = s.trim();
    if let Some(rest) = s.strip_prefix("models/") {
        s = rest.trim();
    }

    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}
