use serde::{Deserialize, Serialize};

/// In-memory user record for authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUser {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
}

/// In-memory API key record for fast authentication lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUserKey {
    pub id: i64,
    pub user_id: i64,
    pub api_key: String,
    pub enabled: bool,
}
