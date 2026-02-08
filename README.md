# gproxy

[简体中文](README_zh.md)

A high-performance multi-provider LLM gateway in Rust, with an embedded admin SPA.

gproxy provides:
- Unified downstream APIs (OpenAI / Claude / Gemini style routes)
- Per-provider and per-credential routing
- OAuth helper flows for supported providers
- Usage aggregation + provider live usage querying
- Built-in admin API + React 19 + Tailwind 4 admin UI at `/`

## Built-in providers

Current built-ins (seeded on first bootstrap):

- `openai`
- `claude`
- `aistudio`
- `vertexexpress`
- `vertex`
- `geminicli`
- `claudecode`
- `codex`
- `antigravity`
- `nvidia`
- `deepseek`

You can also create additional providers of kind `custom` from the admin UI/API.

## Architecture (workspace)

- `apps/gproxy`: runnable server binary (proxy + admin API + embedded UI)
- `crates/gproxy-core`: bootstrap, in-memory state, proxy engine
- `crates/gproxy-router`: HTTP routing (`/` proxy + `/admin`)
- `crates/gproxy-provider-core`: provider abstraction, configs, credential/runtime state
- `crates/gproxy-provider-impl`: built-in provider implementations
- `crates/gproxy-storage`: SeaORM storage + usage persistence
- `apps/gproxy/frontend`: admin SPA source (React 19 + Tailwind 4)

## Quick start (local)

Prerequisites:
- Rust stable toolchain
- Node.js + pnpm (only needed to rebuild frontend assets)

1. Build admin frontend assets

```bash
pnpm -C apps/gproxy/frontend install --frozen-lockfile
pnpm -C apps/gproxy/frontend build
```

2. Run server

```bash
cargo run -p gproxy -- --admin-key your-admin-key
```

3. Open UI

- Admin UI: `http://127.0.0.1:8787/`

Default bind is `0.0.0.0:8787` unless changed by CLI/env/DB merged config.

## Configuration

Global config merge order at startup: `CLI > ENV > DB`, then persisted back to DB.

CLI / ENV (from `gproxy_core::bootstrap::CliArgs`):

- `--dsn` / `GPROXY_DSN` (default: `sqlite://gproxy.db?mode=rwc`)
- `--host` / `GPROXY_HOST` (default after merge: `0.0.0.0`)
- `--port` / `GPROXY_PORT` (default after merge: `8787`)
- `--admin-key` / `GPROXY_ADMIN_KEY` (plaintext input; stored as hash)
- `--proxy` / `GPROXY_PROXY` (optional upstream egress proxy)
- `--event-redact-sensitive` / `GPROXY_EVENT_REDACT_SENSITIVE` (default: `true`)

Notes:
- If `admin_key` is not provided and DB has none, gproxy generates one and prints it once on startup.
- Built-in providers are auto-seeded when missing.

### `custom` provider JSON parameter mask

`custom` providers support `channel_settings.json_param_mask` to null out selected JSON fields right before upstream dispatch.

- Applies only to requests with JSON body (`content-type: application/json`)
- Non-JSON requests are not affected
- Non-existing paths are ignored

Supported path formats (one line per entry):

- Top-level key: `temperature`
- Dot/bracket path: `messages[1].content`
- Wildcard: `messages[*].content`
- JSON Pointer: `/messages/0/content`

Example:

```json
{
  "kind": "custom",
  "channel_settings": {
    "id": "custom-openai",
    "enabled": true,
    "proto": "openai_response",
    "base_url": "https://api.example.com",
    "dispatch": { "ops": [] },
    "count_tokens": "upstream",
    "json_param_mask": [
      "temperature",
      "top_p",
      "messages[*].content"
    ]
  }
}
```

## Authentication model

### Admin (`/admin/...`)

Accepted admin key sources (first match):
- `x-admin-key: <key>`
- `Authorization: Bearer <key>`
- Query `?admin_key=<key>` (useful for browser WebSocket `/admin/events/ws`)

### Downstream proxy (`/v1/...` or `/{provider}/...`)

Accepted user key sources (first match):
- `Authorization: Bearer <key>`
- `x-api-key: <key>`
- `x-goog-api-key: <key>`
- Query `?key=<key>`

On bootstrap, `user0` is created and one user key is inserted using the same admin key hash, so the same plaintext key can be used for early proxy testing.

## API overview

See `route.md` for complete routes.

Main route groups:
- Aggregate proxy routes without provider prefix (e.g. `/v1/chat/completions`, `/v1/models`)
- Provider-prefixed proxy routes (e.g. `/openai/v1/chat/completions`)
- Provider internal helper routes:
  - `GET /{provider}/oauth`
  - `GET /{provider}/oauth/callback`
  - `GET /{provider}/usage?credential_id=<id>`
- Admin routes under `/admin/...` (providers, credentials, users, usage, ws events)

## Admin UI

The admin SPA is served at `/` and assets at `/assets/*`.

Current UI modules include:
- Provider configuration (including `custom` provider editing)
- Credential management (view/edit/delete/enable, runtime status)
- Batch credential import (key/json)
- OAuth assistant for supported providers
- Live usage / quota view per credential
- User & API key management
- Terminal event stream viewer (`/admin/events/ws`)
- i18n (`zh_cn` / `en`)

## Build and release

### Binary

```bash
cargo build --release -p gproxy
```

### Docker image

Build:

```bash
docker build -t gproxy:local .
```

Run (explicit command form):

```bash
docker run --rm -p 8787:8787 \
  -e GPROXY_HOST=0.0.0.0 \
  -e GPROXY_PORT=8787 \
  -e GPROXY_ADMIN_KEY=your-admin-key \
  -e GPROXY_DSN='sqlite:///app/data/gproxy.db?mode=rwc' \
  -v $(pwd)/data:/app/data \
  gproxy:local \
  /usr/local/bin/gproxy --host 0.0.0.0 --port 8787 --admin-key your-admin-key --dsn 'sqlite:///app/data/gproxy.db?mode=rwc'
```

### GitHub Actions

- `.github/workflows/docker.yml`: build/push multi-arch image to GHCR
- `.github/workflows/release-binary.yml`: build release binaries across OS/arch matrix

## Related docs

- `route.md`: routes and behavior notes
- `provider.md`: provider credential/config specifics
- `PLAN.md`: project planning draft

## License

AGPL-3.0-or-later
