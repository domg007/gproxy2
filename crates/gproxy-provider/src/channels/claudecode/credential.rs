use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaudeCodeTokenRefresh<'a> {
    pub access_token: &'a str,
    pub refresh_token: &'a str,
    pub expires_at_unix_ms: u64,
    pub subscription_type: Option<&'a str>,
    pub rate_limit_tier: Option<&'a str>,
    pub user_email: Option<&'a str>,
    pub cookie: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ClaudeCodeCredential {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub subscription_type: String,
    pub rate_limit_tier: String,
    #[serde(alias = "session_key")]
    pub cookie: Option<String>,
    pub user_email: Option<String>,
}

impl ClaudeCodeCredential {
    pub fn apply_token_refresh(&mut self, refresh: ClaudeCodeTokenRefresh<'_>) {
        self.access_token = refresh.access_token.to_string();
        self.refresh_token = refresh.refresh_token.to_string();
        self.expires_at = refresh.expires_at_unix_ms.min(i64::MAX as u64) as i64;

        if let Some(subscription_type) = refresh.subscription_type {
            self.subscription_type = subscription_type.to_string();
        }
        if let Some(rate_limit_tier) = refresh.rate_limit_tier {
            self.rate_limit_tier = rate_limit_tier.to_string();
        }
        if let Some(user_email) = refresh.user_email {
            let email_missing = self
                .user_email
                .as_ref()
                .map(|existing| existing.trim().is_empty())
                .unwrap_or(true);
            if email_missing {
                self.user_email = Some(user_email.to_string());
            }
        }
        if let Some(cookie) = refresh.cookie {
            self.cookie = Some(cookie.to_string());
        }
    }
}
