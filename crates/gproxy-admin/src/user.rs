use gproxy_storage::{
    Scope, StorageWriteEvent, StorageWriteSender, UsageQuery, UsageQueryRow, UsageSummary,
    UserKeyQueryRow, UserKeyWrite,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::AdminApiError;
use crate::{MemoryUser, MemoryUserKey, normalize_user_api_key};

pub fn extract_api_key(provided: Option<&str>) -> Result<&str, AdminApiError> {
    provided.ok_or(AdminApiError::Unauthorized)
}

pub async fn authenticate_user_key(
    users: &[MemoryUser],
    keys: &HashMap<String, MemoryUserKey>,
    api_key: &str,
) -> Result<MemoryUserKey, AdminApiError> {
    let key = keys.get(api_key).ok_or(AdminApiError::Unauthorized)?;
    if !key.enabled {
        return Err(AdminApiError::Unauthorized);
    }
    let user = users
        .iter()
        .find(|item| item.id == key.user_id)
        .ok_or(AdminApiError::Unauthorized)?;
    if !user.enabled {
        return Err(AdminApiError::Unauthorized);
    }
    Ok(key.clone())
}

pub async fn query_my_user_keys(
    users: &[MemoryUser],
    keys: &HashMap<String, MemoryUserKey>,
    api_key: &str,
) -> Result<Vec<UserKeyQueryRow>, AdminApiError> {
    let me = authenticate_user_key(users, keys, api_key).await?;
    let rows: Vec<UserKeyQueryRow> = keys
        .values()
        .filter(|row| row.user_id == me.user_id)
        .map(|row| UserKeyQueryRow {
            id: row.id,
            user_id: row.user_id,
            api_key: row.api_key.clone(),
        })
        .collect();
    Ok(rows)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpsertMyKeyInput {
    pub id: Option<i64>,
    pub api_key: String,
    pub label: Option<String>,
    pub enabled: bool,
}

fn next_local_id(keys: &HashMap<String, MemoryUserKey>) -> i64 {
    keys.values().map(|row| row.id).max().unwrap_or(-1) + 1
}

pub async fn upsert_my_user_key(
    writer: &StorageWriteSender,
    current_api_key: &str,
    users: &[MemoryUser],
    keys: &HashMap<String, MemoryUserKey>,
    input: UpsertMyKeyInput,
) -> Result<UserKeyWrite, AdminApiError> {
    let me = authenticate_user_key(users, keys, current_api_key).await?;
    let normalized_api_key = normalize_user_api_key(me.user_id, input.api_key.as_str())
        .ok_or_else(|| AdminApiError::InvalidInput("api_key cannot be empty".to_string()))?;
    if let Some(existing) = keys.get(normalized_api_key.as_str()) {
        if let Some(target_id) = input.id {
            if existing.id != target_id {
                return Err(AdminApiError::InvalidInput(
                    "user key already exists".to_string(),
                ));
            }
        } else {
            return Err(AdminApiError::InvalidInput(
                "user key already exists".to_string(),
            ));
        }
    }

    let payload = UserKeyWrite {
        id: input.id.unwrap_or_else(|| next_local_id(keys)),
        user_id: me.user_id,
        api_key: normalized_api_key,
        label: input.label,
        enabled: input.enabled,
    };

    writer
        .enqueue(StorageWriteEvent::UpsertUserKey(payload.clone()))
        .await?;
    Ok(payload)
}

pub async fn delete_my_user_key(
    writer: &StorageWriteSender,
    current_api_key: &str,
    users: &[MemoryUser],
    keys: &HashMap<String, MemoryUserKey>,
    key_id: i64,
) -> Result<(), AdminApiError> {
    let me = authenticate_user_key(users, keys, current_api_key).await?;
    let target = keys
        .values()
        .find(|row| row.id == key_id)
        .ok_or_else(|| AdminApiError::NotFound(format!("user_key {key_id}")))?;

    if target.user_id != me.user_id {
        return Err(AdminApiError::Forbidden);
    }

    writer
        .enqueue(StorageWriteEvent::DeleteUserKey { id: key_id })
        .await?;
    Ok(())
}

pub async fn query_my_usages(
    storage: &gproxy_storage::SeaOrmStorage,
    users: &[MemoryUser],
    keys: &HashMap<String, MemoryUserKey>,
    current_api_key: &str,
    mut query: UsageQuery,
) -> Result<Vec<UsageQueryRow>, AdminApiError> {
    let me = authenticate_user_key(users, keys, current_api_key).await?;
    query.user_id = Scope::Eq(me.user_id);
    Ok(storage.query_usages(&query).await?)
}

pub async fn summarize_my_usages(
    storage: &gproxy_storage::SeaOrmStorage,
    users: &[MemoryUser],
    keys: &HashMap<String, MemoryUserKey>,
    current_api_key: &str,
    mut query: UsageQuery,
) -> Result<UsageSummary, AdminApiError> {
    let me = authenticate_user_key(users, keys, current_api_key).await?;
    query.user_id = Scope::Eq(me.user_id);
    Ok(storage.summarize_usages(&query).await?)
}
