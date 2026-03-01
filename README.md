# gproxy

`gproxy` is a Rust-based multi-channel LLM proxy that exposes OpenAI / Claude / Gemini-style APIs through a unified gateway, with a built-in admin console, user/key management, and request/usage auditing.

Chinese version: [README.zh.md](./README.zh.md)

If you want to look at the full docs, click [here](https://gproxy-docs.leenhawk.com/).

## Key Features

- Unified multi-channel gateway: route requests to different upstreams by `channel` (builtin + custom).
- Multi-protocol compatibility: one upstream can accept OpenAI/Claude/Gemini requests (controlled by dispatch rules).
- Credential pool and health states: supports `healthy / partial / dead` with model-level cooldown retry.
- OAuth and API Key support: OAuth channels (Codex, ClaudeCode, GeminiCli, Antigravity) and API Key channels.
- Built-in Web console: available at `/`, supports English and Chinese.
- Observability: records upstream/downstream requests and usage metrics (filterable by user/model/time).
- Async batched storage writes: queue + aggregation to reduce database pressure under load.

## Built-in Channels

| Channel ID | Default Upstream | Auth Type |
|---|---|---|
| `openai` | `https://api.openai.com` | API Key |
| `claude` | `https://api.anthropic.com` | API Key |
| `aistudio` | `https://generativelanguage.googleapis.com` | API Key |
| `vertexexpress` | `https://aiplatform.googleapis.com` | API Key |
| `vertex` | `https://aiplatform.googleapis.com` | GCP service account (builtin object) |
| `geminicli` | `https://cloudcode-pa.googleapis.com` | OAuth (builtin object) |
| `claudecode` | `https://api.anthropic.com` | OAuth/Cookie (builtin object) |
| `codex` | `https://chatgpt.com/backend-api/codex` | OAuth (builtin object) |
| `antigravity` | `https://daily-cloudcode-pa.sandbox.googleapis.com` | OAuth (builtin object) |
| `nvidia` | `https://integrate.api.nvidia.com` | API Key |
| `deepseek` | `https://api.deepseek.com` | API Key |
| custom (for example `mycustom`) | your configured `base_url` | API Key (`secret`) |

## Quick Start

### 1. Prerequisites

- Rust (must support `edition = 2024`)
- SQLite (default DSN uses sqlite)
- Optional: Node.js + `pnpm` (if you want to rebuild the admin frontend)

### 2. Prepare Config

```bash
cp gproxy.example.toml gproxy.toml
```

At minimum, set:

- `global.admin_key`
- at least one enabled channel credential (`credentials.secret` or builtin credential object)

Bootstrap login defaults:

- username: `admin`
- password: value of `global.admin_key`

### 3. Run

```bash
cargo run -p gproxy
```

On startup, gproxy prints:

- listening address (default `http://0.0.0.0:8787`)
- current admin key (`password:`)

> If `./gproxy.toml` does not exist, gproxy starts with in-memory defaults and auto-generates a 16-digit admin key (printed to stdout).

### 4. Minimal Verification

```bash
curl -sS http://127.0.0.1:8787/openai/v1/models \
  -H "x-api-key: <your user key or admin key>"
```

Get a user/admin API key via password login:

```bash
curl -sS http://127.0.0.1:8787/login \
  -H "content-type: application/json" \
  -d '{
    "name": "admin",
    "password": "<your admin_key>"
  }'
```

## Deployment

### Local deployment

#### Binary

1. Download the binary from [Releases](https://github.com/LeenHawk/gproxy/releases).
2. Prepare config:

```bash
cp gproxy.example.toml gproxy.toml
```

3. Run binary:

```bash
./gproxy
```

#### Docker

Build:

```bash
docker build -t gproxy:local .
```

Run:

```bash
docker run --rm -p 8787:8787 \
  -e GPROXY_HOST=0.0.0.0 \
  -e GPROXY_PORT=8787 \
  -e GPROXY_ADMIN_KEY=your-admin-key \
  -e GPROXY_DSN='sqlite:///app/data/gproxy.db?mode=rwc' \
  -v $(pwd)/data:/app/data \
  gproxy:local
```

### Cloud deployment

#### Zeabur

- Template file: [`zeabur.yaml`](./zeabur.yaml)
- You can use the Zeabur button from docs/samples, or import this repository in Zeabur and deploy from the template.
- Required env: `GPROXY_ADMIN_KEY`
- Recommended persistence: mount `/app/data` as a persistent volume.

## Admin Frontend

- Console entry: `GET /`
- Static assets: `/assets/*`
- Frontend build output: `apps/gproxy/frontend/dist`
- Backend embeds `dist` into the binary via `rust-embed`

If you changed frontend code, rebuild first:

```bash
cd apps/gproxy/frontend
pnpm install
pnpm build
cd ../../..
cargo run -p gproxy
```

## Configuration (`gproxy.toml`)

Reference files:

- `gproxy.example.toml` (minimal)
- `gproxy.example.full.toml` (full)

### `global`

| Field | Description |
|---|---|
| `host` | Bind host, default `0.0.0.0` |
| `port` | Bind port, default `8787` |
| `proxy` | Upstream proxy (empty string means disabled) |
| `hf_token` | HuggingFace token (optional for tokenizer download) |
| `hf_url` | HuggingFace base URL, default `https://huggingface.co` |
| `admin_key` | Admin bootstrap credential; used as admin password and admin API key on bootstrap, auto-generated if empty |
| `mask_sensitive_info` | Redact sensitive request/response payloads in logs/events |
| `data_dir` | Data directory, default `./data` |
| `dsn` | Database DSN; if omitted and `data_dir` is changed, sqlite DSN is derived automatically |

### `runtime`

| Field | Default | Description |
|---|---:|---|
| `storage_write_queue_capacity` | `4096` | Storage write queue size |
| `storage_write_max_batch_size` | `1024` | Max events per aggregated storage batch |
| `storage_write_aggregate_window_ms` | `25` | Aggregation window (ms) |

### `channels`

Each channel is declared with `[[channels]]`:

- `id`: channel id (for example `openai`, `claude`, `mycustom`)
- `enabled`: runtime enable switch (`false` disables routing to this channel)
- `settings`: channel settings (must include `base_url`)
- `dispatch`: optional; defaults to channel-specific dispatch table when omitted
- `credentials`: credential list (supports multi-credential retry/fallback)

### `channels.credentials`

Each credential can include:

- `id` / `label`: optional identifiers
- `secret`: for API key channels
- `builtin`: structured credential object for OAuth/service-account channels
- `state`: optional health-state seed

`state.health.kind` supports:

- `healthy`
- `partial` (with model cooldown list)
- `dead`

## CLI and Environment Overrides

Priority: `CLI flags / env vars > gproxy.toml > defaults`

Supported overrides:

- `--config` / `GPROXY_CONFIG_PATH`
- `--host` / `GPROXY_HOST`
- `--port` / `GPROXY_PORT`
- `--proxy` / `GPROXY_PROXY`
- `--admin-key` / `GPROXY_ADMIN_KEY`
- `--mask-sensitive-info` / `GPROXY_MASK_SENSITIVE_INFO`
- `--data-dir` / `GPROXY_DATA_DIR`
- `--dsn` / `GPROXY_DSN`
- `--storage-write-queue-capacity` / `GPROXY_STORAGE_WRITE_QUEUE_CAPACITY`
- `--storage-write-max-batch-size` / `GPROXY_STORAGE_WRITE_MAX_BATCH_SIZE`
- `--storage-write-aggregate-window-ms` / `GPROXY_STORAGE_WRITE_AGGREGATE_WINDOW_MS`

## API Overview

All errors return:

```json
{ "error": "..." }
```

### Auth Headers

- `POST /login` uses JSON body `{ "name": "...", "password": "..." }` and returns `api_key`
- Admin/User APIs (except `/login`): use `x-api-key`
- Provider APIs also accept:
  - `x-api-key`
  - `x-goog-api-key`
  - `Authorization: Bearer ...`
  - Gemini query key `?key=...` (normalized into `x-api-key`)

### Provider Routes

#### 1) Scoped (recommended)

Provider is explicit in path, examples:

- `POST /openai/v1/chat/completions`
- `POST /claude/v1/messages`
- `POST /aistudio/v1beta/models/{model}:generateContent`

#### 2) Unscoped (single unified entry)

Provider is resolved from model prefix:

- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/messages`
- `GET /v1/models`
- `GET /v1/models/{provider}/{model}`

Constraints:

- For OpenAI/Claude-style request bodies, `model` must be `<provider>/<model>`, for example `openai/gpt-4.1`.
- For Gemini target paths, provider must be included, for example `models/aistudio/gemini-2.5-flash:generateContent`.

### OAuth and Upstream Usage

- `GET /{provider}/v1/oauth`
- `GET /{provider}/v1/oauth/callback`
- `GET /{provider}/v1/usage`

OAuth-capable channels: `codex`, `claudecode`, `geminicli`, `antigravity`

### Admin APIs (`/admin/*`)

Main groups:

- Global settings: `/admin/global-settings`, `/admin/global-settings/upsert`
- Config export/import: `/admin/config/export-toml`, `/admin/config/import-toml`
- Self update: `/admin/system/self_update`
- Providers/Credentials/CredentialStatuses: `query/upsert/delete`
- Users: `query/upsert/delete` (`/admin/users/upsert` requires `password`)
- UserKeys: `query/generate/delete`
- Requests: `/admin/requests/upstream/query`, `/admin/requests/downstream/query`
- Usage: `/admin/usages/query`, `/admin/usages/summary`

### User APIs (`/user/*`)

- `POST /user/keys/query`
- `POST /user/keys/generate`
- `POST /user/keys/delete`
- `POST /user/usages/query`
- `POST /user/usages/summary`

## Request Examples

### Scoped OpenAI Chat

```bash
curl -sS http://127.0.0.1:8787/openai/v1/chat/completions \
  -H "x-api-key: <key>" \
  -H "content-type: application/json" \
  -d '{
    "model": "gpt-4.1",
    "messages": [{"role":"user","content":"hello"}],
    "stream": false
  }'
```

### Unscoped OpenAI Chat (model-prefixed routing)

```bash
curl -sS http://127.0.0.1:8787/v1/chat/completions \
  -H "x-api-key: <key>" \
  -H "content-type: application/json" \
  -d '{
    "model": "openai/gpt-4.1",
    "messages": [{"role":"user","content":"hello"}],
    "stream": false
  }'
```

### Scoped Gemini GenerateContent

```bash
curl -sS "http://127.0.0.1:8787/aistudio/v1beta/models/gemini-2.5-flash:generateContent" \
  -H "x-api-key: <key>" \
  -H "content-type: application/json" \
  -d '{
    "contents":[{"role":"user","parts":[{"text":"hello"}]}]
  }'
```

## Architecture

### Workspace Layout

| Path | Responsibility |
|---|---|
| `apps/gproxy` | Executable service entry (Axum + embedded admin frontend) |
| `crates/gproxy-core` | AppState, router orchestration, auth, request execution |
| `crates/gproxy-provider` | channel implementations, retry, OAuth, dispatch, tokenizers |
| `crates/gproxy-middleware` | protocol transform middleware, usage extraction |
| `crates/gproxy-protocol` | OpenAI/Claude/Gemini typed protocol models and transforms |
| `crates/gproxy-storage` | SeaORM storage layer, query models, async write queue |
| `crates/gproxy-admin` | admin/user domain operations |

### Runtime Flow

- On bootstrap:
  - load config and apply CLI/env overrides
  - connect database and sync schema automatically
  - initialize provider registry, credentials, and credential states
  - ensure admin principal (`id=0`) and admin key exist
- On request:
  - authenticate user key
  - route + transform/forward according to dispatch table
  - pick eligible credentials and retry/fallback
  - persist upstream/downstream events and usage records

### Credential States and Cooldown

- `healthy`: available
- `partial`: model-level cooldown
- `dead`: unavailable

Default cooldowns:

- rate limit: `60s`
- transient failure: `15s`

## Testing

Provider smoke/regression scripts:

- `tests/provider/curl_provider.sh`
- `tests/provider/run_channel_regression.sh`

Examples:

```bash
API_KEY='<key>' tests/provider/curl_provider.sh \
  --provider openai \
  --method openai_chat \
  --model gpt-4.1
```

```bash
API_KEY='<key>' tests/provider/run_channel_regression.sh \
  --provider openai \
  --model gpt-5-nano \
  --embedding-model text-embedding-3-small
```

## Common Issues

### 1) `401 unauthorized`

- Ensure `x-api-key` is provided for key-protected routes, and both the key and its owner user are enabled.
- If you don't have a key yet, call `POST /login` with username/password first.

### 2) `403 forbidden` on admin routes

- The key is not owned by the admin user (`id=0`).

### 3) `503 all eligible credentials exhausted`

- Check:
  - whether the channel has any available credential
  - whether credential status is `dead` or currently `partial` for the target model
  - whether upstream keeps returning 429/5xx

### 4) `model must be prefixed as <provider>/...`

- You called an unscoped route without provider-prefixed model.

### 5) Realtime WebSocket unavailable

- `/v1/realtime` is currently not implemented; use `/v1/responses` (HTTP) instead.

## Security Notes

- Set a strong `admin_key` in production.
- Keep `mask_sensitive_info = true` unless you explicitly need full payload visibility for debugging.
- If you configure an outbound proxy, ensure the proxy path is trusted and access-controlled.

## Data and Directories

By default:

- data dir: `./data`
- default DB: `sqlite://./data/gproxy.db?mode=rwc`
- tokenizer cache: `./data/tokenizers`

`gproxy-storage` supports sqlite / mysql / postgres via `dsn`.

## Development Commands

```bash
# backend format/lint/check
cargo fmt
cargo check
cargo clippy --workspace --all-targets

# tests
cargo test --workspace

# run service
cargo run -p gproxy
```

Frontend:

```bash
cd apps/gproxy/frontend
pnpm install
pnpm typecheck
pnpm build
```

## License

This project is licensed under `AGPL-3.0-or-later` (see `LICENSE`).

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=LeenHawk/gproxy&type=Date)](https://star-history.com/#LeenHawk/gproxy&Date)
