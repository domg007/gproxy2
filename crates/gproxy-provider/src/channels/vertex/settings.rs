pub use super::constants::{DEFAULT_BASE_URL, DEFAULT_LOCATION, DEFAULT_TOKEN_URI};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VertexSettings {
    pub base_url: String,
    pub location: String,
    pub token_uri: String,
    pub oauth_token_url: String,
}

impl Default for VertexSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            location: DEFAULT_LOCATION.to_string(),
            token_uri: DEFAULT_TOKEN_URI.to_string(),
            oauth_token_url: DEFAULT_TOKEN_URI.to_string(),
        }
    }
}
