use anyhow::Result;
use gproxy_admin::{MemoryUser, MemoryUserKey};
use gproxy_core::GlobalSettings;
use gproxy_storage::{
    SeaOrmStorage, StorageWriteBatch, StorageWriteEvent, StorageWriteSink, UserKeyWrite, UserWrite,
};
use rand::RngExt as _;

pub(super) async fn seed_admin_principal(
    storage: &SeaOrmStorage,
    user: UserWrite,
    key: UserKeyWrite,
) -> Result<()> {
    let mut batch = StorageWriteBatch::default();
    batch.apply(StorageWriteEvent::UpsertUser(user));
    batch.apply(StorageWriteEvent::UpsertUserKey(key));
    storage
        .write_batch(batch)
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok(())
}

pub(super) fn ensure_admin_principal(
    global: &mut GlobalSettings,
    users: &mut Vec<MemoryUser>,
    keys: &mut std::collections::HashMap<String, MemoryUserKey>,
) -> Result<(UserWrite, UserKeyWrite)> {
    const ADMIN_USER_ID: i64 = 0;
    let admin_user_id = if let Some(existing) = users.iter_mut().find(|row| row.id == ADMIN_USER_ID)
    {
        existing.name = "admin".to_string();
        existing.enabled = true;
        existing.id
    } else {
        let id = ADMIN_USER_ID;
        users.push(MemoryUser {
            id,
            name: "admin".to_string(),
            enabled: true,
        });
        id
    };

    let user_write = UserWrite {
        id: admin_user_id,
        name: "admin".to_string(),
        enabled: true,
    };

    let admin_key = if global.admin_key.trim().is_empty() {
        if let Some(existing) = find_existing_admin_api_key(keys, admin_user_id) {
            global.admin_key = existing.clone();
            existing
        } else {
            let generated = generate_16_digit_admin_key();
            eprintln!("bootstrap: generated admin api key: {generated}");
            global.admin_key = generated.clone();
            generated
        }
    } else {
        let normalized = global.admin_key.trim().to_string();
        global.admin_key = normalized.clone();
        normalized
    };

    let admin_key_id = keys
        .get(admin_key.as_str())
        .map(|row| row.id)
        .unwrap_or_else(|| next_incremental_key_id(keys));

    keys.insert(
        admin_key.clone(),
        MemoryUserKey {
            id: admin_key_id,
            user_id: admin_user_id,
            api_key: admin_key.clone(),
            enabled: true,
        },
    );

    let key_write = UserKeyWrite {
        id: admin_key_id,
        user_id: admin_user_id,
        api_key: admin_key,
        label: Some("bootstrap-admin-key".to_string()),
        enabled: true,
    };

    Ok((user_write, key_write))
}

fn find_existing_admin_api_key(
    keys: &std::collections::HashMap<String, MemoryUserKey>,
    admin_user_id: i64,
) -> Option<String> {
    keys.values()
        .filter(|row| row.user_id == admin_user_id)
        .min_by_key(|row| (!row.enabled, row.id))
        .map(|row| row.api_key.clone())
}

fn next_incremental_key_id(keys: &std::collections::HashMap<String, MemoryUserKey>) -> i64 {
    keys.values().map(|row| row.id).max().unwrap_or(-1) + 1
}

fn generate_16_digit_admin_key() -> String {
    const MIN: u64 = 1_000_000_000_000_000;
    const MAX: u64 = 10_000_000_000_000_000;
    rand::rng().random_range(MIN..MAX).to_string()
}
