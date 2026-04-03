use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

fn default_spoof_emulation() -> String {
    "chrome_136".to_string()
}

fn default_update_source() -> String {
    "github".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSettingsWrite {
    pub host: String,
    pub port: u16,
    pub proxy: Option<String>,
    #[serde(default = "default_spoof_emulation")]
    pub spoof_emulation: String,
    #[serde(default = "default_update_source")]
    pub update_source: String,
    pub admin_key: String,
    pub hf_token: Option<String>,
    pub hf_url: Option<String>,
    pub mask_sensitive_info: bool,
    pub dsn: String,
    pub data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderWrite {
    pub id: i64,
    pub name: String,
    pub channel: String,
    pub settings_json: String,
    pub dispatch_json: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialWrite {
    pub id: i64,
    pub provider_id: i64,
    pub name: Option<String>,
    pub kind: String,
    pub secret_json: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CredentialStatusKey {
    pub credential_id: i64,
    pub channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialStatusWrite {
    pub id: Option<i64>,
    pub credential_id: i64,
    pub channel: String,
    pub health_kind: String,
    pub health_json: Option<String>,
    pub checked_at_unix_ms: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserWrite {
    pub id: i64,
    pub name: String,
    pub password: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserKeyWrite {
    pub id: i64,
    pub user_id: i64,
    pub api_key: String,
    pub label: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownstreamRequestWrite {
    pub trace_id: i64,
    pub at_unix_ms: i64,
    pub internal: bool,
    pub user_id: Option<i64>,
    pub user_key_id: Option<i64>,
    pub request_method: String,
    pub request_headers_json: String,
    pub request_path: String,
    pub request_query: Option<String>,
    pub request_body: Option<Vec<u8>>,
    pub response_status: Option<i32>,
    pub response_headers_json: String,
    pub response_body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamRequestWrite {
    pub downstream_trace_id: Option<i64>,
    pub at_unix_ms: i64,
    pub internal: bool,
    pub provider_id: Option<i64>,
    pub credential_id: Option<i64>,
    pub request_method: String,
    pub request_headers_json: String,
    pub request_url: Option<String>,
    pub request_body: Option<Vec<u8>>,
    pub response_status: Option<i32>,
    pub response_headers_json: String,
    pub response_body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageWrite {
    pub downstream_trace_id: Option<i64>,
    pub at_unix_ms: i64,
    pub provider_id: Option<i64>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageWriteEvent {
    UpsertGlobalSettings(GlobalSettingsWrite),
    UpsertProvider(ProviderWrite),
    DeleteProvider { id: i64 },
    UpsertCredential(CredentialWrite),
    DeleteCredential { id: i64 },
    UpsertCredentialStatus(CredentialStatusWrite),
    DeleteCredentialStatus { id: i64 },
    UpsertUser(UserWrite),
    DeleteUser { id: i64 },
    UpsertUserKey(UserKeyWrite),
    DeleteUserKey { id: i64 },
    UpsertDownstreamRequest(DownstreamRequestWrite),
    UpsertUpstreamRequest(UpstreamRequestWrite),
    UpsertUsage(UsageWrite),
}

#[derive(Debug, Default)]
pub struct StorageWriteBatch {
    pub event_count: usize,
    pub global_settings: Option<GlobalSettingsWrite>,
    pub providers_upsert: HashMap<i64, ProviderWrite>,
    pub providers_delete: HashSet<i64>,
    pub credentials_upsert: HashMap<i64, CredentialWrite>,
    pub credentials_delete: HashSet<i64>,
    pub credential_statuses_upsert: HashMap<CredentialStatusKey, CredentialStatusWrite>,
    pub credential_statuses_delete: HashSet<i64>,
    pub users_upsert: HashMap<i64, UserWrite>,
    pub users_delete: HashSet<i64>,
    pub user_keys_upsert: HashMap<String, UserKeyWrite>,
    pub user_keys_delete: HashSet<i64>,
    pub downstream_requests_upsert: Vec<DownstreamRequestWrite>,
    pub upstream_requests_upsert: Vec<UpstreamRequestWrite>,
    pub usages_upsert: Vec<UsageWrite>,
}

impl StorageWriteBatch {
    pub fn apply(&mut self, event: StorageWriteEvent) {
        self.event_count += 1;
        match event {
            StorageWriteEvent::UpsertGlobalSettings(value) => {
                self.global_settings = Some(value);
            }
            StorageWriteEvent::UpsertProvider(value) => {
                self.providers_delete.remove(&value.id);
                self.providers_upsert.insert(value.id, value);
            }
            StorageWriteEvent::DeleteProvider { id } => {
                self.providers_upsert.remove(&id);
                self.providers_delete.insert(id);
            }
            StorageWriteEvent::UpsertCredential(value) => {
                self.credentials_delete.remove(&value.id);
                self.credentials_upsert.insert(value.id, value);
            }
            StorageWriteEvent::DeleteCredential { id } => {
                self.credentials_upsert.remove(&id);
                self.credentials_delete.insert(id);
            }
            StorageWriteEvent::UpsertCredentialStatus(value) => {
                let key = CredentialStatusKey {
                    credential_id: value.credential_id,
                    channel: value.channel.clone(),
                };
                if let Some(id) = value.id {
                    self.credential_statuses_delete.remove(&id);
                }
                self.credential_statuses_upsert.insert(key, value);
            }
            StorageWriteEvent::DeleteCredentialStatus { id } => {
                self.credential_statuses_upsert
                    .retain(|_, value| value.id != Some(id));
                self.credential_statuses_delete.insert(id);
            }
            StorageWriteEvent::UpsertUser(value) => {
                self.users_delete.remove(&value.id);
                self.users_upsert.insert(value.id, value);
            }
            StorageWriteEvent::DeleteUser { id } => {
                self.users_upsert.remove(&id);
                self.users_delete.insert(id);
            }
            StorageWriteEvent::UpsertUserKey(value) => {
                self.user_keys_delete.remove(&value.id);
                self.user_keys_upsert
                    .retain(|api_key, row| row.id != value.id || api_key == &value.api_key);
                self.user_keys_upsert.insert(value.api_key.clone(), value);
            }
            StorageWriteEvent::DeleteUserKey { id } => {
                self.user_keys_upsert.retain(|_, row| row.id != id);
                self.user_keys_delete.insert(id);
            }
            StorageWriteEvent::UpsertDownstreamRequest(value) => {
                self.downstream_requests_upsert.push(value);
            }
            StorageWriteEvent::UpsertUpstreamRequest(value) => {
                self.upstream_requests_upsert.push(value);
            }
            StorageWriteEvent::UpsertUsage(value) => {
                self.usages_upsert.push(value);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.event_count == 0
    }
}
