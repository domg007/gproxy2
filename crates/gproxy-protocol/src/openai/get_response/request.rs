use serde::{Deserialize, Serialize};

use crate::openai::create_response::types::ResponseInclude;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetResponsePath {
    /// The ID of the response to retrieve.
    pub response_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GetResponseQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<ResponseInclude>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starting_after: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_obfuscation: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetResponseRequest {
    pub path: GetResponsePath,
    #[serde(default)]
    pub query: GetResponseQuery,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_query_fields_as_snake_case() {
        let req = GetResponseRequest {
            path: GetResponsePath {
                response_id: "resp_123".to_string(),
            },
            query: GetResponseQuery {
                include: Some(vec![ResponseInclude::FileSearchCallResults]),
                stream: Some(true),
                starting_after: Some(42),
                include_obfuscation: Some(false),
            },
        };

        let value = serde_json::to_value(&req).expect("serialize get response request");
        assert_eq!(value["path"]["response_id"], "resp_123");
        assert_eq!(value["query"]["starting_after"], 42);
        assert_eq!(value["query"]["include_obfuscation"], false);
    }
}
