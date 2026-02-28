# Transform Module

This directory hosts protocol transformation orchestration for gproxy-protocol.

- Native conversion style: `TryFrom<Source> for Target`
- Internal streaming standard: SSE (`data: ...\n\n`)
- Gemini transport adapter: `sse_to_ndjson_stream`
