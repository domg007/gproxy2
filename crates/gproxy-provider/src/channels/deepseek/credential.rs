use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DeepseekCredential {
    pub api_key: String,
}

impl DeepseekCredential {
    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }
}
