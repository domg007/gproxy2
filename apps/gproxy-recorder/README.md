# gproxy-recorder

HTTP/SOCKS recording proxy for capturing LLM API traffic. Transparently forwards requests while recording request/response pairs to HAR format, including streaming events with timestamps.

## Features

- HTTP forward proxy (`http_proxy` / `https_proxy`)
- HTTPS MITM decryption (self-signed CA)
- Streaming response recording (SSE / NDJSON events with timestamps)
- HAR 1.2 output with `_streaming` extension

## Usage

```bash
# Generate CA certificate (first time)
gproxy-recorder init-ca --output ca.pem

# Start recording proxy
gproxy-recorder record \
  --listen 127.0.0.1:8080 \
  --ca ca.pem \
  --output recording.har

# Point your CLI tool at the proxy
export https_proxy=http://127.0.0.1:8080
export SSL_CERT_FILE=ca.pem
claude chat "hello"

# Ctrl-C to stop — recording.har is written on exit
```

## HAR Streaming Extension

For streaming responses (`text/event-stream`, `application/x-ndjson`), events are stored with relative timestamps:

```json
{
  "response": {
    "content": {
      "mimeType": "text/event-stream",
      "_streaming": {
        "events": [
          { "timestamp_ms": 0, "data": "data: {\"type\":\"message_start\",...}" },
          { "timestamp_ms": 45, "data": "data: {\"type\":\"content_block_delta\",...}" }
        ]
      }
    }
  }
}
```
