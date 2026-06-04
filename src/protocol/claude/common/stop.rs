use serde::{Deserialize, Serialize};

use super::{JsonObject, TypedObject};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StopDetails {
    Refusal(RefusalStopDetails),
    Unknown(TypedObject),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefusalStopDetails {
    pub category: Option<RefusalCategory>,
    pub explanation: Option<String>,
    #[serde(rename = "type")]
    pub type_: RefusalStopDetailsType,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefusalStopDetailsType {
    #[serde(rename = "refusal")]
    Refusal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RefusalCategory {
    Known(RefusalCategoryKnown),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefusalCategoryKnown {
    #[serde(rename = "cyber")]
    Cyber,
    #[serde(rename = "bio")]
    Bio,
}
