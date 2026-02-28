# Transform 模块

本目录承载 gproxy-protocol 的协议转换编排层。

- 原生转换方式：`TryFrom<Source> for Target`
- 内部流式标准：SSE（`data: ...\n\n`）
- Gemini 传输适配：`sse_to_ndjson_stream`
