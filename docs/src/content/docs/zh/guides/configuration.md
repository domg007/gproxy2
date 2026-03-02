---
title: 配置说明
description: 多数据库、原生渠道、自定义渠道与 dispatch 转换配置。
---

## 配置入口

推荐从以下文件开始：

- `gproxy.example.toml`：最小可运行示例
- `gproxy.example.full.toml`：全量字段示例

## 配置优先级

运行时优先级：

`CLI 参数 / 环境变量 > gproxy.toml > 默认值`

常用覆盖项：

- `--config` / `GPROXY_CONFIG_PATH`
- `--host` / `GPROXY_HOST`
- `--port` / `GPROXY_PORT`
- `--proxy` / `GPROXY_PROXY`
- `--admin-key` / `GPROXY_ADMIN_KEY`
- `--mask-sensitive-info` / `GPROXY_MASK_SENSITIVE_INFO`
- `--data-dir` / `GPROXY_DATA_DIR`
- `--dsn` / `GPROXY_DSN`

## 多数据库支持（重点）

`gproxy-storage` 已启用 `sqlite + mysql + postgres` 驱动。你只要改 `global.dsn` 即可切换。

示例：

```toml
# SQLite（默认）
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

| 字段 | 说明 |
|---|---|
| `host` | 监听地址，默认 `0.0.0.0` |
| `port` | 监听端口，默认 `8787` |
| `proxy` | 上游代理；空字符串表示禁用 |
| `hf_token` | 可选，HuggingFace token |
| `hf_url` | HuggingFace 基址，默认 `https://huggingface.co` |
| `admin_key` | 管理员 key；为空时首次可自动生成 |
| `mask_sensitive_info` | 是否在日志/事件中脱敏敏感字段 |
| `data_dir` | 数据目录，默认 `./data` |
| `dsn` | 数据库 DSN（sqlite/mysql/postgres） |

## `runtime`

| 字段 | 默认值 | 说明 |
|---|---:|---|
| `storage_write_queue_capacity` | `4096` | 存储写入队列容量 |
| `storage_write_max_batch_size` | `1024` | 单批次最大写入事件数 |
| `storage_write_aggregate_window_ms` | `25` | 聚合窗口（毫秒） |

## `channels`（原生与自定义）

每个通道使用 `[[channels]]` 声明，常见字段：

- `id`：通道 ID（内置如 `openai`，或自定义如 `mycustom`）
- `enabled`：是否启用
- `settings`：通道配置（通常至少包含 `base_url`）
- `dispatch`：可选协议分发规则
- `credentials`：凭证列表（支持多凭证）

示例：

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

## 内置渠道能力矩阵（重点）

| 渠道 | `id` | OAuth | `/v1/usage` | `secret` 凭证 |
|---|---|---|---|---|
| OpenAI | `openai` | 否 | 否 | 是 |
| Claude | `claude` | 否 | 否 | 是 |
| AiStudio | `aistudio` | 否 | 否 | 是 |
| VertexExpress | `vertexexpress` | 否 | 否 | 是 |
| Vertex | `vertex` | 否 | 否 | 否（service account） |
| GeminiCli | `geminicli` | 是 | 是 | 否（OAuth builtin） |
| ClaudeCode | `claudecode` | 是 | 是 | 否（OAuth/Cookie builtin） |
| Codex | `codex` | 是 | 是 | 否（OAuth builtin） |
| Antigravity | `antigravity` | 是 | 是 | 否（OAuth builtin） |
| Nvidia | `nvidia` | 否 | 否 | 是 |
| Deepseek | `deepseek` | 否 | 否 | 是 |
| Groq | `groq` | 否 | 否 | 是 |

## Claude / ClaudeCode 顶层 cache_control 开关

`claude` 与 `claudecode` 支持该配置项：

- 配置键：`channels.settings.enable_top_level_cache_control`
- 默认值：`false`
- 行为：
  - `true`：对 Claude 消息生成请求自动注入顶层 `"cache_control":{"type":"ephemeral"}`
  - `false`：不做任何改写
- 如果请求体已包含顶层 `cache_control`，gproxy 会保留原值

示例：

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

## 自定义渠道配置示例（重点）

```toml
[[channels]]
id = "mycustom"
enabled = true

[channels.settings]
base_url = "https://api.example.com"

[[channels.credentials]]
secret = "custom-provider-api-key"
```

说明：

- 自定义渠道默认走 `ProviderDispatchTable::default_for_custom()`
- 你也可以在配置里显式提供 `dispatch`，做精细化协议路由

## `channels.credentials`

可用字段：

- `id` / `label`：可读标识
- `secret`：API Key 通道
- `builtin`：OAuth / ServiceAccount 结构化凭证
- `state`：健康状态种子

健康状态类型：

- `healthy`：可用
- `partial`：模型级冷却（部分可用）
- `dead`：不可用

## dispatch 与转换能力

`dispatch` 决定“请求进入后如何被实现”：

- `Passthrough`：原样转发给上游
- `TransformTo`：转换为目标协议再转发
- `Local`：本地实现（例如某些计数能力）
- `Unsupported`：显式不支持

这也是 GPROXY 同时支持多协议入口、多上游原生差异的核心机制。
