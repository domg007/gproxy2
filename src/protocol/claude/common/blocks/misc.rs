use serde::{Deserialize, Serialize};

use super::super::CacheControl;
use super::TextBlock;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerUploadBlock {
    pub file_id: String,
    #[serde(rename = "type")]
    pub type_: ContainerUploadBlockType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContainerUploadBlockType {
    #[serde(rename = "container_upload")]
    ContainerUpload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_content: Option<String>,
    #[serde(rename = "type")]
    pub type_: CompactionBlockType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompactionBlockType {
    #[serde(rename = "compaction")]
    Compaction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidConversationSystemBlock {
    pub content: Vec<TextBlock>,
    #[serde(rename = "type")]
    pub type_: MidConversationSystemBlockType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MidConversationSystemBlockType {
    #[serde(rename = "mid_conv_system")]
    MidConversationSystem,
}
