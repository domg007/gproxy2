use std::future::Future;
use std::pin::Pin;

use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, TransactionTrait,
};
use serde_json::Value as JsonValue;
use time::OffsetDateTime;

use super::SeaOrmStorage;
use super::entities::{
    credential_statuses, credentials, downstream_requests, global_settings, providers,
    upstream_requests, usages, user_keys, users,
};
use crate::write::{
    CredentialStatusWrite, CredentialWrite, DownstreamRequestWrite, GlobalSettingsWrite,
    ProviderWrite, StorageWriteBatch, StorageWriteSink, StorageWriteSinkError,
    UpstreamRequestWrite, UsageWrite, UserKeyWrite, UserWrite,
};

const UPSERT_CHUNK_SIZE: usize = 256;

impl StorageWriteSink for SeaOrmStorage {
    fn write_batch<'a>(
        &'a self,
        batch: StorageWriteBatch,
    ) -> Pin<Box<dyn Future<Output = Result<(), StorageWriteSinkError>> + Send + 'a>> {
        Box::pin(async move { self.apply_storage_write_batch(batch).await })
    }
}

impl SeaOrmStorage {
    async fn apply_storage_write_batch(
        &self,
        batch: StorageWriteBatch,
    ) -> Result<(), StorageWriteSinkError> {
        let now = OffsetDateTime::now_utc();
        let txn = self
            .connection()
            .begin()
            .await
            .map_err(|err| StorageWriteSinkError::new(format!("begin transaction: {err}")))?;

        let StorageWriteBatch {
            event_count: _,
            global_settings,
            providers_upsert,
            providers_delete,
            credentials_upsert,
            credentials_delete,
            credential_statuses_upsert,
            credential_statuses_delete,
            users_upsert,
            users_delete,
            user_keys_upsert,
            user_keys_delete,
            downstream_requests_upsert,
            upstream_requests_upsert,
            usages_upsert,
        } = batch;

        delete_credential_statuses(&txn, credential_statuses_delete).await?;

        if !credentials_delete.is_empty() {
            credentials::Entity::delete_many()
                .filter(credentials::Column::Id.is_in(credentials_delete))
                .exec(&txn)
                .await
                .map_err(|err| StorageWriteSinkError::new(format!("delete credentials: {err}")))?;
        }

        if !providers_delete.is_empty() {
            providers::Entity::delete_many()
                .filter(providers::Column::Id.is_in(providers_delete))
                .exec(&txn)
                .await
                .map_err(|err| StorageWriteSinkError::new(format!("delete providers: {err}")))?;
        }

        if !user_keys_delete.is_empty() {
            user_keys::Entity::delete_many()
                .filter(user_keys::Column::Id.is_in(user_keys_delete))
                .exec(&txn)
                .await
                .map_err(|err| StorageWriteSinkError::new(format!("delete user_keys: {err}")))?;
        }

        if !users_delete.is_empty() {
            users::Entity::delete_many()
                .filter(users::Column::Id.is_in(users_delete))
                .exec(&txn)
                .await
                .map_err(|err| StorageWriteSinkError::new(format!("delete users: {err}")))?;
        }

        if let Some(settings) = global_settings {
            upsert_global_settings(&txn, settings, now).await?;
        }

        upsert_providers(&txn, providers_upsert.into_values(), now).await?;
        upsert_credentials(&txn, credentials_upsert.into_values(), now).await?;
        upsert_credential_statuses(&txn, credential_statuses_upsert.into_values(), now).await?;
        upsert_users(&txn, users_upsert.into_values(), now).await?;
        upsert_user_keys(&txn, user_keys_upsert.into_values(), now).await?;
        upsert_downstream_requests(&txn, downstream_requests_upsert, now).await?;
        upsert_upstream_requests(&txn, upstream_requests_upsert, now).await?;
        upsert_usages(&txn, usages_upsert, now).await?;

        txn.commit()
            .await
            .map_err(|err| StorageWriteSinkError::new(format!("commit transaction: {err}")))?;
        Ok(())
    }
}

async fn upsert_global_settings<C: ConnectionTrait>(
    db: &C,
    settings: GlobalSettingsWrite,
    now: OffsetDateTime,
) -> Result<(), StorageWriteSinkError> {
    let id = 1_i64;
    global_settings::Entity::insert(global_settings::ActiveModel {
        id: Set(id),
        host: Set(settings.host),
        port: Set(i32::from(settings.port)),
        admin_key: Set(settings.admin_key),
        hf_token: Set(settings.hf_token),
        hf_url: Set(settings.hf_url),
        proxy: Set(settings.proxy),
        spoof_emulation: Set(Some(settings.spoof_emulation)),
        update_source: Set(Some(settings.update_source)),
        dsn: Set(settings.dsn),
        data_dir: Set(settings.data_dir),
        mask_sensitive_info: Set(settings.mask_sensitive_info),
        updated_at: Set(now),
    })
    .on_conflict(
        OnConflict::column(global_settings::Column::Id)
            .update_columns([
                global_settings::Column::Host,
                global_settings::Column::Port,
                global_settings::Column::AdminKey,
                global_settings::Column::HfToken,
                global_settings::Column::HfUrl,
                global_settings::Column::Proxy,
                global_settings::Column::SpoofEmulation,
                global_settings::Column::UpdateSource,
                global_settings::Column::Dsn,
                global_settings::Column::DataDir,
                global_settings::Column::MaskSensitiveInfo,
                global_settings::Column::UpdatedAt,
            ])
            .to_owned(),
    )
    .exec(db)
    .await
    .map_err(|err| StorageWriteSinkError::new(format!("upsert global_settings: {err}")))?;
    Ok(())
}

async fn upsert_providers<C: ConnectionTrait>(
    db: &C,
    values: impl IntoIterator<Item = ProviderWrite>,
    now: OffsetDateTime,
) -> Result<(), StorageWriteSinkError> {
    let mut models = Vec::new();
    for item in values {
        let settings_json = parse_json("provider.settings_json", &item.settings_json)?;
        let dispatch_json = parse_json("provider.dispatch_json", &item.dispatch_json)?;
        models.push(providers::ActiveModel {
            id: Set(item.id),
            name: Set(item.name),
            channel: Set(item.channel),
            settings_json: Set(settings_json),
            dispatch_json: Set(dispatch_json),
            enabled: Set(item.enabled),
            created_at: Set(now),
            updated_at: Set(now),
        });
    }
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<_> = iter.by_ref().take(UPSERT_CHUNK_SIZE).collect();
        if chunk.is_empty() {
            break;
        }
        providers::Entity::insert_many(chunk)
            .on_conflict(
                OnConflict::column(providers::Column::Id)
                    .update_columns([
                        providers::Column::Name,
                        providers::Column::Channel,
                        providers::Column::SettingsJson,
                        providers::Column::DispatchJson,
                        providers::Column::Enabled,
                        providers::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await
            .map_err(|err| StorageWriteSinkError::new(format!("upsert providers: {err}")))?;
    }
    Ok(())
}

async fn upsert_credentials<C: ConnectionTrait>(
    db: &C,
    values: impl IntoIterator<Item = CredentialWrite>,
    now: OffsetDateTime,
) -> Result<(), StorageWriteSinkError> {
    let mut models = Vec::new();
    for item in values {
        let settings_json =
            parse_optional_json("credential.settings_json", item.settings_json.as_deref())?;
        let secret_json = parse_json("credential.secret_json", &item.secret_json)?;
        models.push(credentials::ActiveModel {
            id: Set(item.id),
            provider_id: Set(item.provider_id),
            name: Set(item.name),
            kind: Set(item.kind),
            settings_json: Set(settings_json),
            secret_json: Set(secret_json),
            enabled: Set(item.enabled),
            created_at: Set(now),
            updated_at: Set(now),
        });
    }
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<_> = iter.by_ref().take(UPSERT_CHUNK_SIZE).collect();
        if chunk.is_empty() {
            break;
        }
        credentials::Entity::insert_many(chunk)
            .on_conflict(
                OnConflict::column(credentials::Column::Id)
                    .update_columns([
                        credentials::Column::ProviderId,
                        credentials::Column::Name,
                        credentials::Column::Kind,
                        credentials::Column::SettingsJson,
                        credentials::Column::SecretJson,
                        credentials::Column::Enabled,
                        credentials::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await
            .map_err(|err| StorageWriteSinkError::new(format!("upsert credentials: {err}")))?;
    }
    Ok(())
}

async fn upsert_credential_statuses<C: ConnectionTrait>(
    db: &C,
    values: impl IntoIterator<Item = CredentialStatusWrite>,
    now: OffsetDateTime,
) -> Result<(), StorageWriteSinkError> {
    let mut models = Vec::new();
    for item in values {
        let health_json =
            parse_optional_json("credential_status.health_json", item.health_json.as_deref())?;
        let checked_at = item
            .checked_at_unix_ms
            .map(unix_ms_to_datetime)
            .transpose()
            .map_err(|err| {
                StorageWriteSinkError::new(format!(
                    "credential_status.checked_at_unix_ms invalid: {err}"
                ))
            })?;
        models.push(credential_statuses::ActiveModel {
            id: item.id.map_or(NotSet, Set),
            credential_id: Set(item.credential_id),
            channel: Set(item.channel),
            health_kind: Set(item.health_kind),
            health_json: Set(health_json),
            checked_at: Set(checked_at),
            last_error: Set(item.last_error),
            updated_at: Set(now),
        });
    }
    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<_> = iter.by_ref().take(UPSERT_CHUNK_SIZE).collect();
        if chunk.is_empty() {
            break;
        }
        credential_statuses::Entity::insert_many(chunk)
            .on_conflict(
                OnConflict::columns([
                    credential_statuses::Column::CredentialId,
                    credential_statuses::Column::Channel,
                ])
                .update_columns([
                    credential_statuses::Column::HealthKind,
                    credential_statuses::Column::HealthJson,
                    credential_statuses::Column::CheckedAt,
                    credential_statuses::Column::LastError,
                    credential_statuses::Column::UpdatedAt,
                ])
                .to_owned(),
            )
            .exec(db)
            .await
            .map_err(|err| {
                StorageWriteSinkError::new(format!("upsert credential_statuses: {err}"))
            })?;
    }
    Ok(())
}

async fn upsert_users<C: ConnectionTrait>(
    db: &C,
    values: impl IntoIterator<Item = UserWrite>,
    now: OffsetDateTime,
) -> Result<(), StorageWriteSinkError> {
    let models: Vec<_> = values
        .into_iter()
        .map(|item| users::ActiveModel {
            id: Set(item.id),
            name: Set(item.name),
            password: Set(Some(item.password)),
            enabled: Set(item.enabled),
            created_at: Set(now),
            updated_at: Set(now),
        })
        .collect();

    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<_> = iter.by_ref().take(UPSERT_CHUNK_SIZE).collect();
        if chunk.is_empty() {
            break;
        }
        users::Entity::insert_many(chunk)
            .on_conflict(
                OnConflict::column(users::Column::Id)
                    .update_columns([
                        users::Column::Name,
                        users::Column::Password,
                        users::Column::Enabled,
                        users::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await
            .map_err(|err| StorageWriteSinkError::new(format!("upsert users: {err}")))?;
    }
    Ok(())
}

async fn upsert_user_keys<C: ConnectionTrait>(
    db: &C,
    values: impl IntoIterator<Item = UserKeyWrite>,
    now: OffsetDateTime,
) -> Result<(), StorageWriteSinkError> {
    let models: Vec<_> = values
        .into_iter()
        .map(|item| user_keys::ActiveModel {
            id: Set(item.id),
            user_id: Set(item.user_id),
            api_key: Set(item.api_key),
            label: Set(item.label),
            enabled: Set(item.enabled),
            created_at: Set(now),
            updated_at: Set(now),
        })
        .collect();

    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<_> = iter.by_ref().take(UPSERT_CHUNK_SIZE).collect();
        if chunk.is_empty() {
            break;
        }
        user_keys::Entity::insert_many(chunk)
            .on_conflict(
                OnConflict::column(user_keys::Column::Id)
                    .update_columns([
                        user_keys::Column::UserId,
                        user_keys::Column::ApiKey,
                        user_keys::Column::Label,
                        user_keys::Column::Enabled,
                        user_keys::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await
            .map_err(|err| StorageWriteSinkError::new(format!("upsert user_keys: {err}")))?;
    }
    Ok(())
}

async fn upsert_downstream_requests<C: ConnectionTrait>(
    db: &C,
    values: impl IntoIterator<Item = DownstreamRequestWrite>,
    now: OffsetDateTime,
) -> Result<(), StorageWriteSinkError> {
    let mut models = Vec::new();
    for item in values {
        let request_headers_json = parse_json(
            "downstream.request_headers_json",
            &item.request_headers_json,
        )?;
        let response_headers_json = parse_json(
            "downstream.response_headers_json",
            &item.response_headers_json,
        )?;
        let at = unix_ms_to_datetime(item.at_unix_ms).map_err(|err| {
            StorageWriteSinkError::new(format!("downstream.at_unix_ms invalid: {err}"))
        })?;

        models.push(downstream_requests::ActiveModel {
            trace_id: Set(item.trace_id),
            at: Set(at),
            internal: Set(item.internal),
            user_id: Set(item.user_id),
            user_key_id: Set(item.user_key_id),
            request_method: Set(item.request_method),
            request_headers_json: Set(request_headers_json),
            request_path: Set(item.request_path),
            request_query: Set(item.request_query),
            request_body: Set(item.request_body),
            response_status: Set(item.response_status),
            response_headers_json: Set(response_headers_json),
            response_body: Set(item.response_body),
            created_at: Set(now),
        });
    }

    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<_> = iter.by_ref().take(UPSERT_CHUNK_SIZE).collect();
        if chunk.is_empty() {
            break;
        }
        downstream_requests::Entity::insert_many(chunk)
            .exec(db)
            .await
            .map_err(|err| {
                StorageWriteSinkError::new(format!("upsert downstream_requests: {err}"))
            })?;
    }
    Ok(())
}

async fn upsert_upstream_requests<C: ConnectionTrait>(
    db: &C,
    values: impl IntoIterator<Item = UpstreamRequestWrite>,
    now: OffsetDateTime,
) -> Result<(), StorageWriteSinkError> {
    let mut models = Vec::new();
    for item in values {
        let request_headers_json =
            parse_json("upstream.request_headers_json", &item.request_headers_json)?;
        let response_headers_json = parse_json(
            "upstream.response_headers_json",
            &item.response_headers_json,
        )?;
        let at = unix_ms_to_datetime(item.at_unix_ms).map_err(|err| {
            StorageWriteSinkError::new(format!("upstream.at_unix_ms invalid: {err}"))
        })?;

        models.push(upstream_requests::ActiveModel {
            trace_id: NotSet,
            downstream_trace_id: Set(item.downstream_trace_id),
            at: Set(at),
            internal: Set(item.internal),
            provider_id: Set(item.provider_id),
            credential_id: Set(item.credential_id),
            request_method: Set(item.request_method),
            request_headers_json: Set(request_headers_json),
            request_url: Set(item.request_url),
            request_body: Set(item.request_body),
            response_status: Set(item.response_status),
            response_headers_json: Set(response_headers_json),
            response_body: Set(item.response_body),
            created_at: Set(now),
        });
    }

    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<_> = iter.by_ref().take(UPSERT_CHUNK_SIZE).collect();
        if chunk.is_empty() {
            break;
        }
        upstream_requests::Entity::insert_many(chunk)
            .exec(db)
            .await
            .map_err(|err| {
                StorageWriteSinkError::new(format!("upsert upstream_requests: {err}"))
            })?;
    }
    Ok(())
}

async fn upsert_usages<C: ConnectionTrait>(
    db: &C,
    values: impl IntoIterator<Item = UsageWrite>,
    now: OffsetDateTime,
) -> Result<(), StorageWriteSinkError> {
    let mut models = Vec::new();
    for item in values {
        let at = unix_ms_to_datetime(item.at_unix_ms).map_err(|err| {
            StorageWriteSinkError::new(format!("usage.at_unix_ms invalid: {err}"))
        })?;
        models.push(usages::ActiveModel {
            trace_id: NotSet,
            downstream_trace_id: Set(item.downstream_trace_id),
            at: Set(at),
            provider_id: Set(item.provider_id),
            credential_id: Set(item.credential_id),
            user_id: Set(item.user_id),
            user_key_id: Set(item.user_key_id),
            operation: Set(item.operation),
            protocol: Set(item.protocol),
            model: Set(item.model),
            input_tokens: Set(item.input_tokens),
            output_tokens: Set(item.output_tokens),
            cache_read_input_tokens: Set(item.cache_read_input_tokens),
            cache_creation_input_tokens: Set(item.cache_creation_input_tokens),
            cache_creation_input_tokens_5min: Set(item.cache_creation_input_tokens_5min),
            cache_creation_input_tokens_1h: Set(item.cache_creation_input_tokens_1h),
            created_at: Set(now),
        });
    }

    let mut iter = models.into_iter();
    loop {
        let chunk: Vec<_> = iter.by_ref().take(UPSERT_CHUNK_SIZE).collect();
        if chunk.is_empty() {
            break;
        }
        usages::Entity::insert_many(chunk)
            .exec(db)
            .await
            .map_err(|err| StorageWriteSinkError::new(format!("upsert usages: {err}")))?;
    }
    Ok(())
}

fn parse_json(field: &str, raw: &str) -> Result<JsonValue, StorageWriteSinkError> {
    serde_json::from_str(raw)
        .map_err(|err| StorageWriteSinkError::new(format!("invalid json for {field}: {err}")))
}

fn parse_optional_json(
    field: &str,
    raw: Option<&str>,
) -> Result<Option<JsonValue>, StorageWriteSinkError> {
    raw.map(|item| parse_json(field, item)).transpose()
}

fn unix_ms_to_datetime(unix_ms: i64) -> Result<OffsetDateTime, time::error::ComponentRange> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(unix_ms) * 1_000_000)
}

async fn delete_credential_statuses<C: ConnectionTrait>(
    db: &C,
    ids: impl IntoIterator<Item = i64>,
) -> Result<(), StorageWriteSinkError> {
    let mut iter = ids.into_iter();
    loop {
        let chunk: Vec<_> = iter.by_ref().take(UPSERT_CHUNK_SIZE).collect();
        if chunk.is_empty() {
            break;
        }

        credential_statuses::Entity::delete_many()
            .filter(credential_statuses::Column::Id.is_in(chunk))
            .exec(db)
            .await
            .map_err(|err| {
                StorageWriteSinkError::new(format!("delete credential_statuses: {err}"))
            })?;
    }

    Ok(())
}
