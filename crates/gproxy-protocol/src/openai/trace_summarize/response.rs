use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TraceSummarizeOutput {
    pub trace_summary: String,
    pub memory_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TraceSummarizeResponse {
    pub output: Vec<TraceSummarizeOutput>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_trace_summarize_response_payload() {
        let json = r#"
        {
          "output": [
            {
              "trace_summary": "trace summary #1",
              "memory_summary": "memory summary #1"
            },
            {
              "trace_summary": "trace summary #2",
              "memory_summary": "memory summary #2"
            }
          ]
        }
        "#;

        let parsed: TraceSummarizeResponse =
            serde_json::from_str(json).expect("deserialize trace summarize response");
        assert_eq!(parsed.output.len(), 2);
        assert_eq!(parsed.output[0].trace_summary, "trace summary #1");
        assert_eq!(parsed.output[1].memory_summary, "memory summary #2");
    }
}
