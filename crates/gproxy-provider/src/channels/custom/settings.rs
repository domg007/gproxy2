use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CustomChannelSettings {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub mask_table: CustomMaskTable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CustomMaskTable {
    #[serde(default)]
    pub rules: Vec<CustomMaskRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CustomMaskRule {
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub remove_fields: Vec<String>,
}

impl CustomChannelSettings {
    pub fn from_provider_settings_value(
        value: &serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        #[derive(Debug, Clone, Default, Deserialize)]
        #[serde(default)]
        struct ProviderSettingsPatch {
            base_url: String,
            user_agent: Option<String>,
            mask_table: Option<serde_json::Value>,
        }

        let patch = serde_json::from_value::<ProviderSettingsPatch>(value.clone())?;
        let mut settings = Self::default();
        if !patch.base_url.trim().is_empty() {
            settings.base_url = patch.base_url;
        }
        settings.user_agent = patch.user_agent.map(|value| value.trim().to_string());
        if let Some(mask_table) = patch.mask_table.as_ref() {
            settings.mask_table = parse_custom_mask_table(mask_table)?;
        }
        Ok(settings)
    }
}

fn parse_custom_mask_table(
    value: &serde_json::Value,
) -> Result<CustomMaskTable, serde_json::Error> {
    match value {
        serde_json::Value::Null => Ok(CustomMaskTable::default()),
        serde_json::Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Ok(CustomMaskTable::default());
            }
            let parsed = serde_json::from_str::<serde_json::Value>(trimmed)?;
            serde_json::from_value(parsed)
        }
        _ => serde_json::from_value(value.clone()),
    }
}
