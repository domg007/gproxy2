use serde::{Deserialize, Serialize};

pub use crate::openai::types::{
    HttpMethod, OpenAiApiError, OpenAiApiErrorResponse, OpenAiResponseHeaders,
};

/// Provider-specific extension bag for OpenAI-compatible `/embeddings` payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OpenAiEmbeddingExtraBody {
    /// NVIDIA extension: query vs passage embedding behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_type: Option<OpenAiEmbeddingNvidiaInputType>,
    /// NVIDIA extension: overlength handling strategy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncate: Option<OpenAiEmbeddingNvidiaTruncate>,
    /// NVIDIA extension: output embedding precision/format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_type: Option<OpenAiEmbeddingNvidiaEmbeddingType>,
}

/// NVIDIA `input_type` for embedding requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenAiEmbeddingNvidiaInputType {
    #[serde(rename = "query")]
    Query,
    #[serde(rename = "passage")]
    Passage,
}

/// NVIDIA truncation policy for overlength input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenAiEmbeddingNvidiaTruncate {
    #[serde(rename = "NONE")]
    None,
    #[serde(rename = "START")]
    Start,
    #[serde(rename = "END")]
    End,
}

/// NVIDIA compressed embedding output type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenAiEmbeddingNvidiaEmbeddingType {
    #[serde(rename = "float")]
    Float,
    #[serde(rename = "binary")]
    Binary,
    #[serde(rename = "ubinary")]
    Ubinary,
    #[serde(rename = "int8")]
    Int8,
    #[serde(rename = "uint8")]
    Uint8,
}

/// Input union accepted by OpenAI `/embeddings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpenAiEmbeddingInput {
    String(String),
    StringArray(Vec<String>),
    TokenArray(Vec<i64>),
    TokenArrayArray(Vec<Vec<i64>>),
}

/// Supported embedding model names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpenAiEmbeddingModel {
    Known(OpenAiEmbeddingModelKnown),
    Custom(String),
}

/// Known embedding model constants from upstream docs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenAiEmbeddingModelKnown {
    #[serde(rename = "text-embedding-ada-002")]
    TextEmbeddingAda002,
    #[serde(rename = "text-embedding-3-small")]
    TextEmbedding3Small,
    #[serde(rename = "text-embedding-3-large")]
    TextEmbedding3Large,
}

/// Encoding format for returned embedding values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenAiEmbeddingEncodingFormat {
    #[serde(rename = "float")]
    Float,
    #[serde(rename = "base64")]
    Base64,
}

/// A single embedding object in the response list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiEmbeddingData {
    /// Embedding vector, encoded as float list or base64 string.
    pub embedding: OpenAiEmbeddingVector,
    /// Position of this embedding in the request input list.
    pub index: u64,
    /// Object discriminator, always `embedding`.
    pub object: OpenAiEmbeddingDataObject,
}

/// Embedding payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpenAiEmbeddingVector {
    FloatArray(Vec<f64>),
    Base64(String),
}

/// OpenAI embedding item object discriminator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenAiEmbeddingDataObject {
    #[serde(rename = "embedding")]
    Embedding,
}

/// Usage metrics for embeddings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OpenAiEmbeddingUsage {
    pub prompt_tokens: u64,
    pub total_tokens: u64,
}

/// Successful response payload for OpenAI `/embeddings`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiCreateEmbeddingResponse {
    pub data: Vec<OpenAiEmbeddingData>,
    pub model: String,
    pub object: OpenAiEmbeddingResponseObject,
    pub usage: OpenAiEmbeddingUsage,
}

/// OpenAI embeddings response object discriminator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenAiEmbeddingResponseObject {
    #[serde(rename = "list")]
    List,
}
