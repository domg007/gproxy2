use std::future::Future;
use std::pin::Pin;

use super::{Event, EventSink};

/// Best-effort terminal sink for structured events.
///
/// This is intentionally lightweight and does not depend on `tracing`.
/// It prints one JSON line per event.
pub struct TerminalEventSink;

impl TerminalEventSink {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TerminalEventSink {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSink for TerminalEventSink {
    fn write<'a>(&'a self, event: &'a Event) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            // Avoid panicking in sinks.
            match event.to_log_json() {
                Ok(line) => {
                    // Use stderr to keep stdout clean for potential streaming responses.
                    eprintln!("{line}");
                }
                Err(err) => {
                    eprintln!("{{\"event\":\"event_serialize_error\",\"error\":\"{err}\"}}");
                }
            }
        })
    }
}
