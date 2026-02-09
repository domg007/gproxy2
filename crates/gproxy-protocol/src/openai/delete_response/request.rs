use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteResponsePath {
    /// The ID of the response to delete.
    pub response_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteResponseRequest {
    pub path: DeleteResponsePath,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_delete_response_path() {
        let req = DeleteResponseRequest {
            path: DeleteResponsePath {
                response_id: "resp_123".to_string(),
            },
        };

        let value = serde_json::to_value(&req).expect("serialize delete response request");
        assert_eq!(value["path"]["response_id"], "resp_123");
    }
}
