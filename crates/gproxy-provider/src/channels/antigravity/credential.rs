use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AntigravityCredential {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    #[serde(default)]
    pub project_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub user_email: Option<String>,
}
