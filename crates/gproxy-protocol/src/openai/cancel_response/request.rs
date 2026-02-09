use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelResponsePath {
    /// The ID of the response to cancel.
    pub response_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelResponseRequest {
    pub path: CancelResponsePath,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_cancel_response_path() {
        let req = CancelResponseRequest {
            path: CancelResponsePath {
                response_id: "resp_123".to_string(),
            },
        };

        let value = serde_json::to_value(&req).expect("serialize cancel response request");
        assert_eq!(value["path"]["response_id"], "resp_123");
    }
}
