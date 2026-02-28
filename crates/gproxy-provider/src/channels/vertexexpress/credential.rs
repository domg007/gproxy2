use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct VertexExpressCredential {
    pub api_key: String,
}

impl VertexExpressCredential {
    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }
}
