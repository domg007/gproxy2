use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::*;

pub type EmbeddingWireModel = OpenAiWireModel<EmbeddingRequest, EmbeddingResponse>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingRequest {
    pub input: EmbeddingInput,
    pub model: OpenAiModelId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<EmbeddingEncodingFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    Text(String),
    TextList(Vec<String>),
    TokenList(Vec<i64>),
    TokenLists(Vec<Vec<i64>>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub data: Vec<Embedding>,
    pub model: OpenAiModelId,
    pub object: ListObjectType,
    pub usage: EmbeddingUsage,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Embedding {
    pub embedding: Vec<f64>,
    pub index: u32,
    pub object: EmbeddingObjectType,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn embedding_request_rejects_undocumented_encoding_format() {
        let result = serde_json::from_value::<EmbeddingRequest>(json!({
            "input": "hello",
            "model": "text-embedding-3-small",
            "encoding_format": "hex"
        }));

        assert!(result.is_err());
    }

    #[test]
    fn embedding_request_models_documented_encoding_formats() {
        let request: EmbeddingRequest = serde_json::from_value(json!({
            "input": "hello",
            "model": "text-embedding-3-small",
            "encoding_format": "base64"
        }))
        .expect("documented embedding encoding format should deserialize");

        assert_eq!(
            request.encoding_format,
            Some(EmbeddingEncodingFormat::Base64)
        );
        assert!(!request.extra.contains_key("encoding_format"));
    }

    #[test]
    fn embedding_response_rejects_base64_embedding_payload() {
        let result = serde_json::from_value::<EmbeddingResponse>(json!({
            "object": "list",
            "model": "text-embedding-3-small",
            "data": [{
                "object": "embedding",
                "index": 0,
                "embedding": "AAAA"
            }],
            "usage": {
                "prompt_tokens": 1,
                "total_tokens": 1
            }
        }));

        assert!(result.is_err());
    }
}
