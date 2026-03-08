use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GrokCredential {
    pub sso: String,
}

impl GrokCredential {
    pub fn is_configured(&self) -> bool {
        !self.sso.trim().is_empty()
    }
}
