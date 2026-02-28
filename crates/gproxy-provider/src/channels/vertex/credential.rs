use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VertexServiceAccountCredential {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
    pub private_key_id: String,
    pub client_id: String,
    pub auth_uri: Option<String>,
    pub token_uri: Option<String>,
    pub auth_provider_x509_cert_url: Option<String>,
    pub client_x509_cert_url: Option<String>,
    pub universe_domain: Option<String>,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub expires_at: i64,
}

impl Default for VertexServiceAccountCredential {
    fn default() -> Self {
        Self {
            project_id: String::new(),
            client_email: String::new(),
            private_key: String::new(),
            private_key_id: String::new(),
            client_id: String::new(),
            auth_uri: Some("https://accounts.google.com/o/oauth2/auth".to_string()),
            token_uri: Some("https://oauth2.googleapis.com/token".to_string()),
            auth_provider_x509_cert_url: None,
            client_x509_cert_url: None,
            universe_domain: None,
            access_token: String::new(),
            expires_at: 0,
        }
    }
}

impl VertexServiceAccountCredential {
    pub fn has_refresh_material(&self) -> bool {
        !self.project_id.trim().is_empty()
            && !self.client_email.trim().is_empty()
            && !self.private_key.trim().is_empty()
    }
}
