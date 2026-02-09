use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::provider::UpstreamTransportErrorKind;
use crate::{CredentialId, Headers, UnavailableReason, UsageSummary};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    Downstream(DownstreamEvent),
    Upstream(UpstreamEvent),
    Operational(OperationalEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownstreamEvent {
    pub trace_id: Option<String>,
    pub at: SystemTime,
    pub user_id: Option<i64>,
    pub user_key_id: Option<i64>,
    pub request_method: String,
    pub request_headers: Headers,
    pub request_path: String,
    pub request_query: Option<String>,
    pub request_body: Option<Vec<u8>>,
    pub response_status: Option<u16>,
    pub response_headers: Headers,
    pub response_body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamEvent {
    pub trace_id: Option<String>,
    pub at: SystemTime,
    pub user_id: Option<i64>,
    pub user_key_id: Option<i64>,
    pub provider: String,
    pub credential_id: Option<CredentialId>,
    pub internal: bool,
    pub attempt_no: u32,
    pub operation: String,
    pub request_method: String,
    pub request_headers: Headers,
    pub request_path: String,
    pub request_query: Option<String>,
    pub request_body: Option<Vec<u8>>,
    pub response_status: Option<u16>,
    pub response_headers: Headers,
    pub response_body: Option<Vec<u8>>,
    pub usage: Option<UsageSummary>,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
    pub transport_kind: Option<UpstreamTransportErrorKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationalEvent {
    UnavailableStart(UnavailableStartEvent),
    UnavailableEnd(UnavailableEndEvent),
    ModelUnavailableStart(ModelUnavailableStartEvent),
    ModelUnavailableEnd(ModelUnavailableEndEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnavailableStartEvent {
    pub at: SystemTime,
    pub credential_id: CredentialId,
    pub reason: UnavailableReason,
    pub until: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnavailableEndEvent {
    pub at: SystemTime,
    pub credential_id: CredentialId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUnavailableStartEvent {
    pub at: SystemTime,
    pub credential_id: CredentialId,
    pub model: String,
    pub reason: UnavailableReason,
    pub until: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUnavailableEndEvent {
    pub at: SystemTime,
    pub credential_id: CredentialId,
    pub model: String,
}

impl Event {
    pub fn to_log_value(&self) -> Result<JsonValue, serde_json::Error> {
        let mut value = serde_json::to_value(self)?;
        match self {
            Event::Downstream(evt) => {
                if let Some(obj) = value
                    .get_mut("Downstream")
                    .and_then(JsonValue::as_object_mut)
                {
                    obj.insert(
                        "request_body".to_string(),
                        bytes_body_to_json(&evt.request_body),
                    );
                    obj.insert(
                        "response_body".to_string(),
                        bytes_body_to_json(&evt.response_body),
                    );
                }
            }
            Event::Upstream(evt) => {
                if let Some(obj) = value.get_mut("Upstream").and_then(JsonValue::as_object_mut) {
                    obj.insert(
                        "request_body".to_string(),
                        bytes_body_to_json(&evt.request_body),
                    );
                    obj.insert(
                        "response_body".to_string(),
                        bytes_body_to_json(&evt.response_body),
                    );
                }
            }
            Event::Operational(_) => {}
        }
        Ok(value)
    }

    pub fn to_log_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.to_log_value()?)
    }
}

fn bytes_body_to_json(body: &Option<Vec<u8>>) -> JsonValue {
    match body {
        Some(bytes) => JsonValue::String(String::from_utf8_lossy(bytes).to_string()),
        None => JsonValue::Null,
    }
}
