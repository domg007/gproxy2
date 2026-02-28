use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GeminiCliCredential {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    #[serde(default)]
    pub project_id: String,
    pub client_id: String,
    pub client_secret: String,
    pub user_email: Option<String>,
}

impl GeminiCliCredential {
    pub fn apply_token_refresh(
        &mut self,
        access_token: &str,
        refresh_token: Option<&str>,
        expires_at_unix_ms: u64,
        user_email: Option<&str>,
    ) {
        self.access_token = access_token.to_string();
        if let Some(refresh_token) = refresh_token {
            self.refresh_token = refresh_token.to_string();
        }
        self.expires_at = expires_at_unix_ms.min(i64::MAX as u64) as i64;

        if let Some(user_email) = user_email {
            let email_missing = self
                .user_email
                .as_ref()
                .map(|existing| existing.trim().is_empty())
                .unwrap_or(true);
            if email_missing {
                self.user_email = Some(user_email.to_string());
            }
        }
    }
}
