pub use super::constants::DEFAULT_BASE_URL;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroqSettings {
    pub base_url: String,
}

impl Default for GroqSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }
}
