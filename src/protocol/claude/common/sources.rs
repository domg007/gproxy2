use serde::{Deserialize, Serialize};

use super::TypedObject;
use super::blocks::{ImageBlock, TextBlock};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Source {
    Base64(Base64Source),
    Url(UrlSource),
    File(FileSource),
    Text(TextSource),
    Content(ContentSource),
    Raw(TypedObject),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Base64Source {
    pub data: String,
    pub media_type: String,
    #[serde(rename = "type")]
    pub type_: Base64SourceType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Base64SourceType {
    #[serde(rename = "base64")]
    Base64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UrlSource {
    pub url: String,
    #[serde(rename = "type")]
    pub type_: UrlSourceType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UrlSourceType {
    #[serde(rename = "url")]
    Url,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSource {
    pub file_id: String,
    #[serde(rename = "type")]
    pub type_: FileSourceType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FileSourceType {
    #[serde(rename = "file")]
    File,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSource {
    pub data: String,
    pub media_type: String,
    #[serde(rename = "type")]
    pub type_: TextSourceType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TextSourceType {
    #[serde(rename = "text")]
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentSource {
    pub content: ContentSourceContent,
    #[serde(rename = "type")]
    pub type_: ContentSourceType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContentSourceType {
    #[serde(rename = "content")]
    Content,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentSourceContent {
    Text(String),
    Blocks(Vec<ContentSourceBlock>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentSourceBlock {
    Text(TextBlock),
    Image(ImageBlock),
    Raw(TypedObject),
}
