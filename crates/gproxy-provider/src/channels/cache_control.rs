use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TopLevelCacheControlMode {
    #[default]
    Disabled,
    Auto,
    Ttl5m,
    Ttl1h,
}

impl TopLevelCacheControlMode {
    pub fn from_settings_value(value: Option<&Value>) -> Self {
        let Some(Value::String(raw)) = value else {
            return Self::Disabled;
        };

        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Self::Disabled,
            "auto" => Self::Auto,
            "5m" => Self::Ttl5m,
            "1h" => Self::Ttl1h,
            _ => Self::Disabled,
        }
    }

    pub fn as_provider_settings_value(self) -> Option<Value> {
        match self {
            Self::Disabled => None,
            Self::Auto => Some(Value::String("auto".to_string())),
            Self::Ttl5m => Some(Value::String("5m".to_string())),
            Self::Ttl1h => Some(Value::String("1h".to_string())),
        }
    }

    pub fn ttl(self) -> Option<&'static str> {
        match self {
            Self::Ttl5m => Some("5m"),
            Self::Ttl1h => Some("1h"),
            _ => None,
        }
    }

    pub fn is_enabled(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}
