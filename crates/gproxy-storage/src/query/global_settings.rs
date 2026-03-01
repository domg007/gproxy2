use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalSettingsRow {
    pub id: i64,
    pub host: String,
    pub port: i32,
    pub admin_key: String,
    pub hf_token: Option<String>,
    pub hf_url: Option<String>,
    pub proxy: Option<String>,
    pub spoof_emulation: Option<String>,
    pub dsn: String,
    pub data_dir: String,
    pub mask_sensitive_info: bool,
    pub updated_at: OffsetDateTime,
}
