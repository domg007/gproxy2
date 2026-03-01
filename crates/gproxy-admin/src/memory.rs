use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryUser {
    pub id: i64,
    pub name: String,
    pub password: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryUserKey {
    pub id: i64,
    pub user_id: i64,
    pub api_key: String,
    pub enabled: bool,
}

pub fn normalize_user_api_key(api_key: &str) -> Option<String> {
    let raw = api_key.trim();
    if raw.is_empty() {
        return None;
    }
    Some(raw.to_string())
}

pub fn generate_user_api_key() -> String {
    Uuid::now_v7().as_simple().to_string()
}
