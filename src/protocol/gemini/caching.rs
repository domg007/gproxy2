use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::{
    CachedContentName, DurationString, ExtraFields, FieldMask, GeminiModelName, JsonMap,
    Rfc3339Timestamp,
};
use super::content::Content;
use super::tools::{Tool, ToolConfig};

pub type CreateCachedContentRequestBody = CachedContent;
pub type CreateCachedContentResponseBody = CachedContent;
pub type GetCachedContentResponseBody = CachedContent;
pub type UpdateCachedContentRequestBody = CachedContent;
pub type UpdateCachedContentResponseBody = CachedContent;
pub type DeleteCachedContentResponseBody = JsonMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CachedContent {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contents: Vec<Content>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<Rfc3339Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<Rfc3339Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<CachedContentUsageMetadata>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub expiration: Option<CachedContentExpiration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<CachedContentName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<GeminiModelName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<ToolConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CachedContentExpiration {
    ExpireTime {
        #[serde(rename = "expireTime")]
        expire_time: Rfc3339Timestamp,
    },
    Ttl {
        ttl: DurationString,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CachedContentUsageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_token_count: Option<i32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListCachedContentsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListCachedContentsResponseBody {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cached_contents: Vec<CachedContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetCachedContentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<CachedContentName>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCachedContentQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_mask: Option<FieldMask>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCachedContentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<CachedContentName>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}
