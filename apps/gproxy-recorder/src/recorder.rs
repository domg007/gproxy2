use std::sync::{Arc, Mutex};

use crate::har::{
    Har, HarContent, HarEntry, HarHeader, HarPostData, HarQueryParam, HarRequest, HarResponse,
    HarStreamEvent, HarStreamingContent,
};

/// Accumulates recorded HTTP entries.
pub struct Recorder {
    entries: Arc<Mutex<Vec<HarEntry>>>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Record a completed request/response pair.
    #[allow(clippy::too_many_arguments)]
    pub fn record_entry(
        &self,
        method: &str,
        url: &str,
        http_version: &str,
        request_headers: Vec<(String, String)>,
        request_body: Option<String>,
        request_content_type: Option<String>,
        status: u16,
        status_text: &str,
        response_headers: Vec<(String, String)>,
        response_body: Option<String>,
        response_content_type: Option<String>,
        streaming_events: Option<Vec<HarStreamEvent>>,
        started: chrono::DateTime<chrono::Utc>,
        elapsed_ms: f64,
    ) {
        let query_string = parse_query_string(url);

        let har_request_headers = to_har_headers(request_headers);
        let har_response_headers = to_har_headers(response_headers);

        let body_size = request_body.as_ref().map_or(0, |b| b.len() as i64);
        let post_data = request_body.map(|text| HarPostData {
            mime_type: request_content_type.unwrap_or_default(),
            text,
        });

        let response_body_size = response_body.as_ref().map_or(0, |b| b.len() as i64);
        let response_size = response_body.as_ref().map_or(0, |b| b.len() as i64);

        let streaming = streaming_events.map(|events| HarStreamingContent { events });

        let entry = HarEntry {
            started_date_time: started.to_rfc3339(),
            time: elapsed_ms,
            request: HarRequest {
                method: method.to_string(),
                url: url.to_string(),
                http_version: http_version.to_string(),
                headers: har_request_headers,
                query_string,
                body_size,
                post_data,
            },
            response: HarResponse {
                status,
                status_text: status_text.to_string(),
                http_version: http_version.to_string(),
                headers: har_response_headers,
                content: HarContent {
                    size: response_size,
                    mime_type: response_content_type.unwrap_or_default(),
                    text: response_body,
                    _streaming: streaming,
                },
                body_size: response_body_size,
            },
        };

        self.entries.lock().unwrap().push(entry);
    }

    /// Serialize all recorded entries to a HAR JSON file.
    pub fn flush_to_file(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let entries = {
            let guard = self.entries.lock().unwrap();
            guard.clone()
        };

        let har = Har::new();
        let har = Har {
            log: crate::har::HarLog { entries, ..har.log },
        };

        let json = serde_json::to_string_pretty(&har)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn entry_count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

fn to_har_headers(headers: Vec<(String, String)>) -> Vec<HarHeader> {
    headers
        .into_iter()
        .map(|(name, value)| HarHeader { name, value })
        .collect()
}

fn parse_query_string(url: &str) -> Vec<HarQueryParam> {
    if let Some(query_start) = url.find('?') {
        let query = &url[query_start + 1..];
        query
            .split('&')
            .filter_map(|param| {
                let mut parts = param.splitn(2, '=');
                let name = parts.next()?.to_string();
                let value = parts.next().unwrap_or("").to_string();
                Some(HarQueryParam { name, value })
            })
            .collect()
    } else {
        Vec::new()
    }
}
