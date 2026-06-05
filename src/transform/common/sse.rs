/// A provider-neutral SSE transport frame.
///
/// This is framing, not a model event IR. Pair modules still own event payload
/// conversion after JSON decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFrame {
    pub event: Option<String>,
    pub data: String,
}

impl SseFrame {
    pub fn data(data: impl Into<String>) -> Self {
        Self {
            event: None,
            data: data.into(),
        }
    }

    pub fn event(event: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            event: Some(event.into()),
            data: data.into(),
        }
    }

    pub fn encode(&self) -> String {
        let mut encoded = String::new();
        if let Some(event) = &self.event {
            encoded.push_str("event: ");
            encoded.push_str(event);
            encoded.push('\n');
        }
        for line in self.data.lines() {
            encoded.push_str("data: ");
            encoded.push_str(line);
            encoded.push('\n');
        }
        encoded.push('\n');
        encoded
    }
}
