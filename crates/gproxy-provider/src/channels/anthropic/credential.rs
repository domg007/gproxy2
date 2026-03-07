use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AnthropicCredential {
    pub api_key: String,
}

impl AnthropicCredential {
    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }
}
