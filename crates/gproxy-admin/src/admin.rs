use gproxy_storage::{
    CredentialQuery, CredentialQueryCount, CredentialQueryRow, CredentialStatusQuery,
    CredentialStatusQueryCount, CredentialStatusQueryRow, CredentialStatusWrite, CredentialWrite,
    DownstreamRequestQuery, DownstreamRequestQueryRow, GlobalSettingsRow, GlobalSettingsWrite,
    ProviderQuery, ProviderQueryRow, ProviderWrite, RequestQueryCount, Scope, StorageWriteEvent,
    StorageWriteSender, UpstreamRequestQuery, UpstreamRequestQueryRow, UsageQuery, UsageQueryCount,
    UsageQueryRow, UsageSummary, UserKeyQuery, UserKeyQueryRow, UserKeyWrite, UserQuery,
    UserQueryRow, UserWrite,
};
use std::collections::HashMap;

use crate::error::AdminApiError;
use crate::{MemoryUser, MemoryUserKey};

pub async fn get_global_settings(
    storage: &gproxy_storage::SeaOrmStorage,
) -> Result<Option<GlobalSettingsRow>, AdminApiError> {
    Ok(storage.get_global_settings().await?)
}

pub async fn upsert_global_settings(
    writer: &StorageWriteSender,
    payload: GlobalSettingsWrite,
) -> Result<(), AdminApiError> {
    writer
        .enqueue(StorageWriteEvent::UpsertGlobalSettings(payload))
        .await?;
    Ok(())
}

pub async fn query_providers(
    storage: &gproxy_storage::SeaOrmStorage,
    query: ProviderQuery,
) -> Result<Vec<ProviderQueryRow>, AdminApiError> {
    Ok(storage.list_providers(&query).await?)
}

pub async fn upsert_provider(
    writer: &StorageWriteSender,
    payload: ProviderWrite,
) -> Result<(), AdminApiError> {
    writer
        .enqueue(StorageWriteEvent::UpsertProvider(payload))
        .await?;
    Ok(())
}

pub async fn delete_provider(writer: &StorageWriteSender, id: i64) -> Result<(), AdminApiError> {
    writer
        .enqueue(StorageWriteEvent::DeleteProvider { id })
        .await?;
    Ok(())
}

pub async fn query_credentials(
    storage: &gproxy_storage::SeaOrmStorage,
    query: CredentialQuery,
) -> Result<Vec<CredentialQueryRow>, AdminApiError> {
    Ok(storage.list_credentials(&query).await?)
}

pub async fn count_credentials(
    storage: &gproxy_storage::SeaOrmStorage,
    query: CredentialQuery,
) -> Result<CredentialQueryCount, AdminApiError> {
    Ok(storage.count_credentials(&query).await?)
}

pub async fn upsert_credential(
    writer: &StorageWriteSender,
    payload: CredentialWrite,
) -> Result<(), AdminApiError> {
    writer
        .enqueue(StorageWriteEvent::UpsertCredential(payload))
        .await?;
    Ok(())
}

pub async fn delete_credential(writer: &StorageWriteSender, id: i64) -> Result<(), AdminApiError> {
    writer
        .enqueue(StorageWriteEvent::DeleteCredential { id })
        .await?;
    Ok(())
}

pub async fn query_credential_statuses(
    storage: &gproxy_storage::SeaOrmStorage,
    query: CredentialStatusQuery,
) -> Result<Vec<CredentialStatusQueryRow>, AdminApiError> {
    Ok(storage.list_credential_statuses(&query).await?)
}

pub async fn count_credential_statuses(
    storage: &gproxy_storage::SeaOrmStorage,
    query: CredentialStatusQuery,
) -> Result<CredentialStatusQueryCount, AdminApiError> {
    Ok(storage.count_credential_statuses(&query).await?)
}

pub async fn upsert_credential_status(
    writer: &StorageWriteSender,
    payload: CredentialStatusWrite,
) -> Result<(), AdminApiError> {
    writer
        .enqueue(StorageWriteEvent::UpsertCredentialStatus(payload))
        .await?;
    Ok(())
}

pub async fn delete_credential_status(
    writer: &StorageWriteSender,
    id: i64,
) -> Result<(), AdminApiError> {
    writer
        .enqueue(StorageWriteEvent::DeleteCredentialStatus { id })
        .await?;
    Ok(())
}

pub async fn query_users(
    users: &[MemoryUser],
    query: UserQuery,
) -> Result<Vec<UserQueryRow>, AdminApiError> {
    let mut rows: Vec<UserQueryRow> = users
        .iter()
        .map(|user| UserQueryRow {
            id: user.id,
            name: user.name.clone(),
            password: user.password.clone(),
            enabled: user.enabled,
        })
        .collect();
    if let Scope::Eq(id) = query.id {
        rows.retain(|row| row.id == id);
    }
    if let Scope::Eq(name) = &query.name {
        rows.retain(|row| &row.name == name);
    }
    Ok(rows)
}

pub async fn upsert_user(
    writer: &StorageWriteSender,
    payload: UserWrite,
) -> Result<(), AdminApiError> {
    writer
        .enqueue(StorageWriteEvent::UpsertUser(payload))
        .await?;
    Ok(())
}

pub async fn delete_user(writer: &StorageWriteSender, id: i64) -> Result<(), AdminApiError> {
    writer.enqueue(StorageWriteEvent::DeleteUser { id }).await?;
    Ok(())
}

pub async fn query_user_keys(
    keys: &HashMap<String, MemoryUserKey>,
    query: UserKeyQuery,
) -> Result<Vec<UserKeyQueryRow>, AdminApiError> {
    let mut rows: Vec<UserKeyQueryRow> = keys
        .values()
        .map(|key| UserKeyQueryRow {
            id: key.id,
            user_id: key.user_id,
            api_key: key.api_key.clone(),
        })
        .collect();
    if let Scope::Eq(id) = query.id {
        rows.retain(|row| row.id == id);
    }
    if let Scope::Eq(user_id) = query.user_id {
        rows.retain(|row| row.user_id == user_id);
    }
    if let Scope::Eq(api_key) = &query.api_key {
        rows.retain(|row| &row.api_key == api_key);
    }
    Ok(rows)
}

pub async fn upsert_user_key(
    keys: &HashMap<String, MemoryUserKey>,
    writer: &StorageWriteSender,
    mut payload: UserKeyWrite,
) -> Result<UserKeyWrite, AdminApiError> {
    let existing_by_id = keys.values().find(|row| row.id == payload.id);
    payload.api_key = if let Some(existing) = existing_by_id {
        existing.api_key.clone()
    } else {
        crate::generate_unique_user_api_key(keys)?
    };
    writer
        .enqueue(StorageWriteEvent::UpsertUserKey(payload.clone()))
        .await?;
    Ok(payload)
}

pub async fn delete_user_key(writer: &StorageWriteSender, id: i64) -> Result<(), AdminApiError> {
    writer
        .enqueue(StorageWriteEvent::DeleteUserKey { id })
        .await?;
    Ok(())
}

pub async fn query_upstream_requests(
    storage: &gproxy_storage::SeaOrmStorage,
    query: UpstreamRequestQuery,
) -> Result<Vec<UpstreamRequestQueryRow>, AdminApiError> {
    Ok(storage.query_upstream_requests(&query).await?)
}

pub async fn query_downstream_requests(
    storage: &gproxy_storage::SeaOrmStorage,
    query: DownstreamRequestQuery,
) -> Result<Vec<DownstreamRequestQueryRow>, AdminApiError> {
    Ok(storage.query_downstream_requests(&query).await?)
}

pub async fn count_upstream_requests(
    storage: &gproxy_storage::SeaOrmStorage,
    query: UpstreamRequestQuery,
) -> Result<RequestQueryCount, AdminApiError> {
    Ok(storage.count_upstream_requests(&query).await?)
}

pub async fn count_downstream_requests(
    storage: &gproxy_storage::SeaOrmStorage,
    query: DownstreamRequestQuery,
) -> Result<RequestQueryCount, AdminApiError> {
    Ok(storage.count_downstream_requests(&query).await?)
}

pub async fn clear_upstream_request_payloads(
    storage: &gproxy_storage::SeaOrmStorage,
    trace_ids: Option<&[i64]>,
) -> Result<u64, AdminApiError> {
    Ok(storage.clear_upstream_request_payloads(trace_ids).await?)
}

pub async fn clear_downstream_request_payloads(
    storage: &gproxy_storage::SeaOrmStorage,
    trace_ids: Option<&[i64]>,
) -> Result<u64, AdminApiError> {
    Ok(storage.clear_downstream_request_payloads(trace_ids).await?)
}

pub async fn delete_upstream_requests(
    storage: &gproxy_storage::SeaOrmStorage,
    trace_ids: Option<&[i64]>,
) -> Result<u64, AdminApiError> {
    Ok(storage.delete_upstream_requests(trace_ids).await?)
}

pub async fn delete_downstream_requests(
    storage: &gproxy_storage::SeaOrmStorage,
    trace_ids: Option<&[i64]>,
) -> Result<u64, AdminApiError> {
    Ok(storage.delete_downstream_requests(trace_ids).await?)
}

pub async fn query_usages(
    storage: &gproxy_storage::SeaOrmStorage,
    query: UsageQuery,
) -> Result<Vec<UsageQueryRow>, AdminApiError> {
    Ok(storage.query_usages(&query).await?)
}

pub async fn summarize_usages(
    storage: &gproxy_storage::SeaOrmStorage,
    query: UsageQuery,
) -> Result<UsageSummary, AdminApiError> {
    Ok(storage.summarize_usages(&query).await?)
}

pub async fn count_usages(
    storage: &gproxy_storage::SeaOrmStorage,
    query: UsageQuery,
) -> Result<UsageQueryCount, AdminApiError> {
    Ok(storage.count_usages(&query).await?)
}

pub fn default_user_query() -> UserQuery {
    UserQuery {
        id: Scope::All,
        name: Scope::All,
    }
}
