use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::openai::create_response::types::Reasoning;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MemoryTraceMetadata {
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MemoryTrace {
    pub id: String,
    pub metadata: MemoryTraceMetadata,
    /// Normalized trace items consumed by the summarize endpoint.
    pub items: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TraceSummarizeRequestBody {
    /// Model ID used to summarize traces into memory snapshots.
    pub model: String,
    pub traces: Vec<MemoryTrace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceSummarizeRequest {
    pub body: TraceSummarizeRequestBody,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::create_response::types::ReasoningEffort;

    #[test]
    fn serializes_trace_summarize_request_payload() {
        let req = TraceSummarizeRequest {
            body: TraceSummarizeRequestBody {
                model: "gpt-5-codex".to_string(),
                traces: vec![MemoryTrace {
                    id: "trace_1".to_string(),
                    metadata: MemoryTraceMetadata {
                        source_path: "/tmp/trace-1.json".to_string(),
                    },
                    items: vec![serde_json::json!({
                        "type": "message",
                        "role": "assistant",
                        "content": []
                    })],
                }],
                reasoning: Some(Reasoning {
                    effort: Some(ReasoningEffort::Low),
                    summary: None,
                    generate_summary: None,
                }),
            },
        };

        let value = serde_json::to_value(&req).expect("serialize trace summarize request");
        assert_eq!(value["body"]["model"], "gpt-5-codex");
        assert_eq!(value["body"]["traces"][0]["metadata"]["source_path"], "/tmp/trace-1.json");
        assert_eq!(value["body"]["reasoning"]["effort"], "low");
    }
}
