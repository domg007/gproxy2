use crate::gemini::generate_content::response::ResponseBody as GeminiGenerateContentResponseBody;
use crate::gemini::stream_generate_content::stream::{
    GeminiNdjsonStreamBody, GeminiSseEvent, GeminiSseEventData, GeminiSseStreamBody,
};
use crate::gemini::types::JsonObject;
use crate::stream::sse_to_ndjson_stream;

pub fn parse_json_object_or_empty(input: &str) -> JsonObject {
    serde_json::from_str::<JsonObject>(input).unwrap_or_default()
}

pub fn chunk_event(body: GeminiGenerateContentResponseBody) -> GeminiSseEvent {
    GeminiSseEvent {
        event: None,
        data: GeminiSseEventData::Chunk(body),
    }
}

pub fn done_event() -> GeminiSseEvent {
    GeminiSseEvent {
        event: None,
        data: GeminiSseEventData::Done("[DONE]".to_string()),
    }
}

pub fn sse_body_to_ndjson_body(sse_body: &GeminiSseStreamBody) -> GeminiNdjsonStreamBody {
    let mut sse_text = String::new();
    for event in &sse_body.events {
        let payload = match &event.data {
            GeminiSseEventData::Chunk(chunk) => match serde_json::to_string(chunk) {
                Ok(text) => text,
                Err(_) => continue,
            },
            GeminiSseEventData::Done(done) => done.clone(),
        };

        sse_text.push_str("data: ");
        sse_text.push_str(&payload);
        sse_text.push_str("\n\n");
    }

    let ndjson_text = sse_to_ndjson_stream(&sse_text);
    let chunks = ndjson_text
        .lines()
        .filter_map(|line| serde_json::from_str::<GeminiGenerateContentResponseBody>(line).ok())
        .collect::<Vec<_>>();

    GeminiNdjsonStreamBody { chunks }
}
