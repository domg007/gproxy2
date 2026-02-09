use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeleteResponseObjectType {
    #[serde(rename = "response")]
    Response,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DeleteResponseResponse {
    pub id: String,
    pub object: DeleteResponseObjectType,
    pub deleted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_delete_response_payload() {
        let json = r#"
        {
          "id": "resp_6786a1bec27481909a17d673315b29f6",
          "object": "response",
          "deleted": true
        }
        "#;

        let parsed: DeleteResponseResponse =
            serde_json::from_str(json).expect("deserialize delete response payload");
        assert_eq!(parsed.id, "resp_6786a1bec27481909a17d673315b29f6");
        assert!(parsed.deleted);
    }
}
