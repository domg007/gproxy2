use crate::openai::create_response::response::Response;

pub type GetResponseResponse = Response;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_get_response_payload() {
        let json = r#"
        {
          "id": "resp_123",
          "object": "response",
          "created_at": 1741386163,
          "model": "gpt-4.1",
          "output": []
        }
        "#;

        let parsed: GetResponseResponse =
            serde_json::from_str(json).expect("deserialize get response payload");
        assert_eq!(parsed.id, "resp_123");
        assert_eq!(parsed.model, "gpt-4.1");
        assert!(parsed.output.is_empty());
    }
}
