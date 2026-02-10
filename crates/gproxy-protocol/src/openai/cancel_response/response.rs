use crate::openai::create_response::response::Response;

pub type CancelResponseResponse = Response;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_cancel_response_payload() {
        let json = r#"
        {
          "id": "resp_67cb71b351908190a308f3859487620d06981a8637e6bc44",
          "object": "response",
          "created_at": 1741386163,
          "status": "cancelled",
          "model": "gpt-4o-2024-08-06",
          "output": []
        }
        "#;

        let parsed: CancelResponseResponse =
            serde_json::from_str(json).expect("deserialize cancel response payload");
        assert_eq!(
            parsed.id,
            "resp_67cb71b351908190a308f3859487620d06981a8637e6bc44"
        );
        assert_eq!(
            parsed.status,
            Some(crate::openai::create_response::types::ResponseStatus::Cancelled)
        );
    }
}
