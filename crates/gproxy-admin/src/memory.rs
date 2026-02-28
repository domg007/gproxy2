use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryUser {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryUserKey {
    pub id: i64,
    pub user_id: i64,
    pub api_key: String,
    pub enabled: bool,
}

pub fn normalize_user_api_key(user_id: i64, api_key: &str) -> Option<String> {
    let raw = api_key.trim();
    if raw.is_empty() {
        return None;
    }
    let prefix = format!("u{user_id}_");
    if raw.starts_with(prefix.as_str()) {
        return Some(raw.to_string());
    }
    Some(format!("{prefix}{raw}"))
}
