use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Har {
    pub log: HarLog,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarLog {
    pub version: String,
    pub creator: HarCreator,
    pub entries: Vec<HarEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarCreator {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarEntry {
    pub started_date_time: String,
    pub time: f64,
    pub request: HarRequest,
    pub response: HarResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarRequest {
    pub method: String,
    pub url: String,
    pub http_version: String,
    pub headers: Vec<HarHeader>,
    pub query_string: Vec<HarQueryParam>,
    pub body_size: i64,
    pub post_data: Option<HarPostData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarResponse {
    pub status: u16,
    pub status_text: String,
    pub http_version: String,
    pub headers: Vec<HarHeader>,
    pub content: HarContent,
    pub body_size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarContent {
    pub size: i64,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _streaming: Option<HarStreamingContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarStreamingContent {
    pub events: Vec<HarStreamEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarStreamEvent {
    pub timestamp_ms: u64,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarQueryParam {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarPostData {
    pub mime_type: String,
    pub text: String,
}

impl Har {
    pub fn new() -> Self {
        Self {
            log: HarLog {
                version: "1.2".to_string(),
                creator: HarCreator {
                    name: "gproxy-recorder".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                },
                entries: Vec::new(),
            },
        }
    }
}
