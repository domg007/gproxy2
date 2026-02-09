use serde::{Deserialize, Serialize};

use crate::openai::create_response::types::ResponseInclude;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListInputItemsPath {
    /// The ID of the response to retrieve input items for.
    pub response_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ListOrder {
    #[serde(rename = "asc")]
    Asc,
    #[serde(rename = "desc")]
    Desc,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ListInputItemsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<ListOrder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<ResponseInclude>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListInputItemsRequest {
    pub path: ListInputItemsPath,
    #[serde(default)]
    pub query: ListInputItemsQuery,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_list_input_items_query_fields_as_snake_case() {
        let req = ListInputItemsRequest {
            path: ListInputItemsPath {
                response_id: "resp_123".to_string(),
            },
            query: ListInputItemsQuery {
                limit: Some(20),
                order: Some(ListOrder::Desc),
                after: Some("msg_100".to_string()),
                include: Some(vec![ResponseInclude::MessageOutputTextLogprobs]),
            },
        };

        let value = serde_json::to_value(&req).expect("serialize list input items request");
        assert_eq!(value["path"]["response_id"], "resp_123");
        assert_eq!(value["query"]["limit"], 20);
        assert_eq!(value["query"]["order"], "desc");
    }
}
