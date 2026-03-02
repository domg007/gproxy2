---
title: Configuration
description: Multi-database, native channels, custom channels, and dispatch conversion settings.
---

## Config entry points

Start from these files:

- `gproxy.example.toml`: minimum runnable example
- `gproxy.example.full.toml`: full field reference

## Override priority

Runtime priority:

`CLI args / env vars > gproxy.toml > defaults`

Common overrides:

- `--config` / `GPROXY_CONFIG_PATH`
- `--host` / `GPROXY_HOST`
- `--port` / `GPROXY_PORT`
- `--proxy` / `GPROXY_PROXY`
- `--admin-key` / `GPROXY_ADMIN_KEY`
- `--mask-sensitive-info` / `GPROXY_MASK_SENSITIVE_INFO`
- `--data-dir` / `GPROXY_DATA_DIR`
- `--dsn` / `GPROXY_DSN`

## Multi-database support (key)

`gproxy-storage` enables `sqlite + mysql + postgres`. Switch backend by changing `global.dsn`.

Examples:

```toml
# SQLite (default)
dsn = "sqlite://./data/gproxy.db?mode=rwc"
```

```toml
# MySQL
dsn = "mysql://user:password@127.0.0.1:3306/gproxy"
```

```toml
# PostgreSQL
dsn = "postgres://user:password@127.0.0.1:5432/gproxy"
```

## `global`

| Field | Description |
|---|---|
| `host` | Listen address, default `0.0.0.0` |
| `port` | Listen port, default `8787` |
| `proxy` | Upstream proxy; empty string means disabled |
| `hf_token` | Optional HuggingFace token |
| `hf_url` | HuggingFace base URL, default `https://huggingface.co` |
| `admin_key` | Admin key; can be auto-generated on first run |
| `mask_sensitive_info` | Mask sensitive fields in logs/events |
| `data_dir` | Data directory, default `./data` |
| `dsn` | DB DSN (sqlite/mysql/postgres) |

## `runtime`

| Field | Default | Description |
|---|---:|---|
| `storage_write_queue_capacity` | `4096` | Storage write queue capacity |
| `storage_write_max_batch_size` | `1024` | Max events per write batch |
| `storage_write_aggregate_window_ms` | `25` | Aggregation window in ms |

## `channels` (native and custom)

Define each channel with `[[channels]]`:

- `id`: channel ID (built-in like `openai`, or custom like `mycustom`)
- `enabled`: whether enabled
- `settings`: channel settings (usually includes `base_url`)
- `dispatch`: optional protocol dispatch rules
- `credentials`: credential list (supports multiple credentials)

Example:

```toml
[[channels]]
id = "openai"
enabled = true

[channels.settings]
base_url = "https://api.openai.com"

[[channels.credentials]]
id = "openai-main"
label = "primary"
secret = "sk-replace-me"
```

## Built-in channel capability matrix (key)

| Channel | `id` | OAuth | `/v1/usage` | `secret` credential |
|---|---|---|---|---|
| OpenAI | `openai` | No | No | Yes |
| Claude | `claude` | No | No | Yes |
| AiStudio | `aistudio` | No | No | Yes |
| VertexExpress | `vertexexpress` | No | No | Yes |
| Vertex | `vertex` | No | No | No (service account) |
| GeminiCli | `geminicli` | Yes | Yes | No (OAuth builtin) |
| ClaudeCode | `claudecode` | Yes | Yes | No (OAuth/Cookie builtin) |
| Codex | `codex` | Yes | Yes | No (OAuth builtin) |
| Antigravity | `antigravity` | Yes | Yes | No (OAuth builtin) |
| Nvidia | `nvidia` | No | No | Yes |
| Deepseek | `deepseek` | No | No | Yes |
| Groq | `groq` | No | No | Yes |

## Claude / ClaudeCode top-level cache control

`claude` and `claudecode` support this settings flag:

- key: `channels.settings.enable_top_level_cache_control`
- default: `false`
- effect:
  - `true`: auto inject top-level `"cache_control":{"type":"ephemeral"}` for Claude message generation requests
  - `false`: do nothing
- if request already has top-level `cache_control`, gproxy preserves the original value

Example:

```toml
[[channels]]
id = "claude"
enabled = true

[channels.settings]
base_url = "https://api.anthropic.com"
enable_top_level_cache_control = true

[[channels]]
id = "claudecode"
enabled = true

[channels.settings]
base_url = "https://api.anthropic.com"
enable_top_level_cache_control = true
```

## Custom channel example (key)

```toml
[[channels]]
id = "mycustom"
enabled = true

[channels.settings]
base_url = "https://api.example.com"

[[channels.credentials]]
secret = "custom-provider-api-key"
```

Notes:

- Custom channels use `ProviderDispatchTable::default_for_custom()` by default.
- You can explicitly provide `dispatch` for fine-grained protocol routing.

## `channels.credentials`

Available fields:

- `id` / `label`: human-readable identifiers
- `secret`: API key credential
- `builtin`: structured OAuth / ServiceAccount credential
- `state`: health status seed

Health status types:

- `healthy`: available
- `partial`: model-level cooldown (partially available)
- `dead`: unavailable

## Dispatch and conversion

`dispatch` defines how a request is executed:

- `Passthrough`: forward as-is
- `TransformTo`: transform to target protocol then forward
- `Local`: local implementation
- `Unsupported`: explicitly unsupported

This is the core mechanism that enables multiple protocol entrances across heterogeneous upstream providers.
