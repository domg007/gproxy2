use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::CreateMessageResponseBody;
use crate::protocol::claude::common::{
    ContentBlock, JsonObject, StopReason, StopSequence, TypedObject, Usage,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StreamEvent {
    Known(Box<KnownStreamEvent>),
    Unknown(TypedObject),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum KnownStreamEvent {
    #[serde(rename = "message_start")]
    MessageStart {
        message: Box<CreateMessageResponseBody>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: JsonObject,
    },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u64,
        content_block: Box<ContentBlock>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: JsonObject,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: u64,
        delta: EventDelta,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: JsonObject,
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop {
        index: u64,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: JsonObject,
    },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: Box<MessageDelta>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Box<Usage>>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: JsonObject,
    },
    #[serde(rename = "message_stop")]
    MessageStop {
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: JsonObject,
    },
    #[serde(rename = "ping")]
    Ping {
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: JsonObject,
    },
    #[serde(rename = "error")]
    Error {
        error: StreamError,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: JsonObject,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventDelta {
    Known(KnownEventDelta),
    Unknown(TypedObject),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum KnownEventDelta {
    #[serde(rename = "text_delta")]
    Text { text: String },
    #[serde(rename = "input_json_delta")]
    InputJson { partial_json: String },
    #[serde(rename = "thinking_delta")]
    Thinking { thinking: String },
    #[serde(rename = "signature_delta")]
    Signature { signature: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<StopSequence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_details: Option<TypedObject>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamError {
    #[serde(rename = "type")]
    pub type_: String,
    pub message: String,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: JsonObject,
}
