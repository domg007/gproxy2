# Provider Upstream Curl Tests

This folder contains curl-based smoke tests for provider upstream access.

## Detailed Plan

- `tests/provider/TEST_PLAN.md` (uses the approved model mapping for all channels)

## Script

- `tests/provider/curl_provider.sh`
- `tests/provider/run_channel_regression.sh` (incremental, per-provider)

## Required

- Running `gproxy` server
- Valid API key (header `x-api-key`)

## Parameters

- `--provider`: provider id (for example `openai`, `anthropic`, `aistudio`, `vertexexpress`, `vertex`, `geminicli`, `claudecode`, `codex`, `antigravity`, `nvidia`, `deepseek`, or custom channel id)
- `--method`: test method (see list below)
- `--model`: model name (required for content/model_get methods)
- `--embedding-model`: optional OpenAI embedding model for incremental regression
- `--gemini-embedding-model`: optional Gemini embedding model for incremental regression
- `--image-model`: optional OpenAI image generation model for incremental regression
- `--image-edit-model`: optional OpenAI image edit model for incremental regression
- `--video-model`: optional OpenAI video model for incremental regression
- `--prompt`: optional content prompt
- `--api-key`: API key (or `API_KEY` env)
- `--base-url`: default `http://127.0.0.1:8787`
- `--timeout`: default `120` seconds
- `--verbose`: print curl command details

Default prompt:

- `explain what is AI in 100 words`

## Supported Methods

- `openai_model_list`
- `openai_model_get`
- `claude_model_list`
- `claude_model_get`
- `gemini_model_list`
- `gemini_model_get`
- `openai_chat`
- `openai_chat_stream`
- `openai_responses`
- `openai_responses_stream`
- `openai_input_tokens`
- `openai_image_generate`
- `openai_image_generate_stream`
- `openai_image_edit`
- `openai_image_edit_stream`
- `openai_video_create`
- `openai_embeddings`
- `embeddings` (alias of `openai_embeddings`)
- `openai_compact`
- `compact` (alias of `openai_compact`)
- `claude_messages`
- `claude_messages_stream`
- `claude_count_tokens`
- `gemini_generate`
- `gemini_stream_generate` (alias of `gemini_stream_generate_sse`)
- `gemini_stream_generate_sse`
- `gemini_stream_generate_ndjson`
- `gemini_count_tokens`
- `gemini_embeddings`

## Examples

```bash
# OpenAI chat completion via provider channel
API_KEY='your-key' tests/provider/curl_provider.sh \
  --provider openai \
  --method openai_chat \
  --model gpt-4.1
```

```bash
# Claude messages
API_KEY='your-key' tests/provider/curl_provider.sh \
  --provider anthropic \
  --method claude_messages \
  --model claude-3-5-sonnet-20241022
```

```bash
# Gemini generate content (model can omit models/ prefix)
API_KEY='your-key' tests/provider/curl_provider.sh \
  --provider aistudio \
  --method gemini_generate \
  --model gemini-2.5-flash
```

```bash
# Gemini stream generate as NDJSON (no alt=sse)
API_KEY='your-key' tests/provider/curl_provider.sh \
  --provider aistudio \
  --method gemini_stream_generate_ndjson \
  --model gemini-2.5-flash
```

```bash
# Gemini native embeddings
API_KEY='your-key' tests/provider/curl_provider.sh \
  --provider aistudio \
  --method gemini_embeddings \
  --model gemini-embedding-001
```

```bash
# OpenAI responses stream
API_KEY='your-key' tests/provider/curl_provider.sh \
  --provider codex \
  --method openai_responses_stream \
  --model gpt-4.1
```

```bash
# Claude messages stream
API_KEY='your-key' tests/provider/curl_provider.sh \
  --provider anthropic \
  --method claude_messages_stream \
  --model claude-3-5-sonnet-20241022
```

```bash
# OpenAI compact
API_KEY='your-key' tests/provider/curl_provider.sh \
  --provider codex \
  --method compact \
  --model gpt-4.1
```

```bash
# Positional shortcut
API_KEY='your-key' tests/provider/curl_provider.sh openai model_list
```

## Incremental Per-Provider Regression

This avoids re-running completed methods and burning quota.

Results are written per provider:

- `tests/result/<provider>.md`

Example:

```bash
API_KEY='your-key' tests/provider/run_channel_regression.sh \
  --provider openai \
  --model gpt-5-nano \
  --embedding-model text-embedding-3-small \
  --image-model gpt-image-1 \
  --image-edit-model chatgpt-image-latest \
  --video-model sora-2
```

## OpenAI Channel Lessons Learned

These points came from real OpenAI-channel regression and should be reused for later channels.

1. Always bypass local proxy for localhost checks.
- Use `--noproxy '*'` for `127.0.0.1`/`localhost`, otherwise local tests can show fake `503`.
- `tests/provider/curl_provider.sh` now auto-adds `--noproxy '*'` for local base URLs.

2. Do not patch dispatch to hide transform/protocol bugs.
- If a protocol conversion fails (for example OpenAI responses -> Claude), fix compatibility in transform/protocol structs.
- Keep dispatch semantics stable.

3. Separate functional failures from quota/rate-limit failures.
- `429` upstream can lead to `all eligible credentials exhausted` and then `503`.
- Treat this as quota/rate-limit (`blocked`) instead of a protocol implementation failure.

4. Stream tests need stream-aware assertions.
- A plain curl transfer error alone is not enough for verdict.
- Prefer checking HTTP status and presence of valid SSE/NDJSON events.

5. Keep goals and result files aligned with actual acceptance scope.
- If a method is intentionally out of current acceptance (for example OpenAI embeddings with exhausted quota), remove it from that channel’s goal checklist.

6. Use incremental runs only.
- Continue using `tests/provider/run_channel_regression.sh` or targeted methods to avoid re-consuming model quota on already-passed items.
