# gproxy

`gproxy` 是一个基于 Rust 的多通道 LLM 代理服务，统一对外提供 OpenAI / Claude / Gemini 风格接口，并内置管理后台、用户与密钥管理、请求与用量审计。

假如你打算查看完整文档，查看[此处](https://gproxy.leenhawk.com/).

## 主要能力

- 多通道统一入口：按 `channel` 路由到不同上游（内置 + 自定义）。
- 多协议转换：同一上游可接收 OpenAI/Claude/Gemini 不同协议请求（由 dispatch 决定）。
- 凭证池与健康状态：支持 `healthy / partial / dead`，并按模型级冷却重试。
- OAuth 与 API Key 并存：支持 OAuth 类通道（Codex、ClaudeCode、GeminiCli、Antigravity）和 API Key 类通道。
- 管理后台（Web UI）：根路径 `/` 直接访问控制台，支持中英文。
- 可观测性：记录 upstream/downstream 请求与 usage 统计（可按用户、模型、时间过滤）。
- 异步批量写库：写入通过队列聚合，降低高并发下数据库压力。

## 内置通道

| Channel ID | 默认上游 | 认证方式 |
|---|---|---|
| `openai` | `https://api.openai.com` | API Key |
| `claude` | `https://api.anthropic.com` | API Key |
| `aistudio` | `https://generativelanguage.googleapis.com` | API Key |
| `vertexexpress` | `https://aiplatform.googleapis.com` | API Key |
| `vertex` | `https://aiplatform.googleapis.com` | GCP Service Account（builtin 对象） |
| `geminicli` | `https://cloudcode-pa.googleapis.com` | OAuth（builtin 对象） |
| `claudecode` | `https://api.anthropic.com` | OAuth/Cookie（builtin 对象） |
| `codex` | `https://chatgpt.com/backend-api/codex` | OAuth（builtin 对象） |
| `antigravity` | `https://daily-cloudcode-pa.sandbox.googleapis.com` | OAuth（builtin 对象） |
| `nvidia` | `https://integrate.api.nvidia.com` | API Key |
| `deepseek` | `https://api.deepseek.com` | API Key |
| 自定义 `mycustom` | 你配置的 `base_url` | API Key（`secret`） |

## 快速开始

### 1. 准备依赖

- Rust（需支持 `edition = 2024`）
- SQLite（默认 DSN 使用 sqlite）
- 可选：Node.js + `pnpm`（如果你要重新构建管理前端）

### 2. 准备配置

```bash
cp gproxy.example.toml gproxy.toml
```

至少填写：

- `global.admin_key`
- 至少一个启用通道的 `credentials.secret`（或 builtin 凭证对象）

首次登录默认关系：

- 用户名：`admin`
- 密码：`global.admin_key` 的值

### 3. 启动

```bash
cargo run -p gproxy
```

启动后会打印：

- 监听地址（默认 `http://127.0.0.1:8787`）
- 当前 admin key（`password:`）

> 如果 `./gproxy.toml` 不存在，服务会使用内存默认配置启动，并自动生成一个 16 位 admin key（打印在控制台）。

### 4. 最小验证

```bash
curl -sS http://127.0.0.1:8787/openai/v1/models \
  -H "x-api-key: <你的用户key或admin key>"
```

也可以先通过用户名密码换取 `api_key`：

```bash
curl -sS http://127.0.0.1:8787/login \
  -H "content-type: application/json" \
  -d '{
    "name": "admin",
    "password": "<你的 admin_key>"
  }'
```

## 部署

### 本地部署

#### 二进制

1. 从 [Releases](https://github.com/LeenHawk/gproxy/releases) 下载对应平台二进制。
2. 准备配置文件：

```bash
cp gproxy.example.toml gproxy.toml
```

3. 启动二进制：

```bash
./gproxy
```

#### Docker

拉取预构建镜像（推荐）：

```bash
docker pull ghcr.io/leenhawk/gproxy:latest
```

本地源码构建（仅在你需要运行本地改动时）：

```bash
docker build -t gproxy:local .
```

运行：

```bash
docker run --rm -p 8787:8787 \
  -e GPROXY_HOST=0.0.0.0 \
  -e GPROXY_PORT=8787 \
  -e GPROXY_ADMIN_KEY=your-admin-key \
  -e GPROXY_DSN='sqlite:///app/data/gproxy.db?mode=rwc' \
  -v $(pwd)/data:/app/data \
  ghcr.io/leenhawk/gproxy:latest
```

### 云端部署

#### ClawCloud Run

[![Run on ClawCloud](https://raw.githubusercontent.com/ClawCloud/Run-Template/refs/heads/main/Run-on-ClawCloud.svg)](https://template.run.claw.cloud/?openapp=system-fastdeploy%3FtemplateName%3Dgproxy)

- 模板文件：[`claw.yaml`](./claw.yaml)
- 可在 ClawCloud Run 的 App Store -> My Apps -> Debugging 中将 `claw.yaml` 作为自定义模板使用。
- 关键输入项：`admin_key`（默认自动生成）、`proxy_url`、`rust_log`、`volume_size`
- 建议将 `/app/data` 挂载为持久化卷。

#### Release 下载与自更新（Cloudflare Pages）

- 发布流程会把签名后的二进制和更新清单发布到独立的 Cloudflare Pages 下载站。
- 默认公开地址：`https://download-gproxy.leenhawk.com`
- 会生成以下清单：
  - `/manifest.json` —— 文档下载页使用的全量索引
  - `/releases/manifest.json` —— 正式版自更新源
  - `/staging/manifest.json` —— 预览版自更新源
- 管理后台里的 `Cloudflare 源` 和 `/admin/system/self_update` 都读取这个下载站。
- 下载站部署所需 GitHub Actions secrets：
  - `CLOUDFLARE_API_TOKEN`
  - `CLOUDFLARE_ACCOUNT_ID`
  - `CLOUDFLARE_DOWNLOADS_PROJECT_NAME`
- 可选 secrets：
  - `DOWNLOAD_PUBLIC_BASE_URL` —— 文档和 manifest 中对外暴露的自定义域名或 Pages 地址
  - `UPDATE_SIGNING_KEY_ID` —— manifest 中的签名 key id 覆盖值（默认 `gproxy-release-v1`）
  - `UPDATE_SIGNING_PRIVATE_KEY_B64` 与 `UPDATE_SIGNING_PUBLIC_KEY_B64` —— 用于 checksum 签名生成与校验

## 前端控制台

- 控制台入口：`GET /`
- 静态资源：`/assets/*`
- 前端构建产物目录：`apps/gproxy/frontend/dist`
- 后端通过 `rust-embed` 把 `dist` 打进二进制

如果你改了前端代码，请先构建：

```bash
cd apps/gproxy/frontend
pnpm install
pnpm build
cd ../../..
cargo run -p gproxy
```

## 配置说明（`gproxy.toml`）

完整示例见：

- `gproxy.example.toml`（最小）
- `gproxy.example.full.toml`（全量）

### `global`

| 字段 | 说明 |
|---|---|
| `host` | 监听地址，默认 `127.0.0.1` |
| `port` | 监听端口，默认 `8787` |
| `proxy` | 上游代理（空字符串表示不使用） |
| `hf_token` | HuggingFace token（本地 tokenizer 下载时可用） |
| `hf_url` | HuggingFace 基址，默认 `https://huggingface.co` |
| `admin_key` | 管理员启动凭据；启动时会作为 admin 密码与 admin API Key，留空则首次自动生成 |
| `mask_sensitive_info` | 是否在日志/事件存储中隐藏敏感请求与响应体 |
| `data_dir` | 数据目录，默认 `./data` |
| `dsn` | 数据库 DSN；若未设置且改了 `data_dir`，会自动派生 sqlite DSN |

### `runtime`

| 字段 | 默认值 | 说明 |
|---|---:|---|
| `storage_write_queue_capacity` | `4096` | 存储写队列长度 |
| `storage_write_max_batch_size` | `1024` | 单批最大聚合写入事件数 |
| `storage_write_aggregate_window_ms` | `25` | 聚合窗口（毫秒） |

### `channels`

每个通道通过 `[[channels]]` 声明：

- `id`: 通道 ID（如 `openai`、`claude`、`mycustom`）
- `enabled`: 是否启用（`false` 时运行时不会路由到该通道）
- `settings`: 通道配置（至少包含 `base_url`）
- `dispatch`: 可选；不填则用该通道默认 dispatch
- `credentials`: 凭证列表（支持多凭证轮询/回退）

### Claude/ClaudeCode 缓存改写（`cache_breakpoints`）

`claude` 与 `claudecode` 通过以下配置控制 cache-control 改写：

- 配置键：`channels.settings.cache_breakpoints`
- 最多 4 条规则
- 目标：`top_level`（别名 `global`）、`tools`、`system`、`messages`
- `ttl`：`auto` / `5m` / `1h`（`auto` 表示注入时不写 ttl 字段）
- 请求体已有 `cache_control` 会始终保留，并计入 4 条上限

无 ttl 的默认值说明：

- `claudecode`：上游默认 `1h`
- `claude`：上游默认 `5m`
- 需要确定性行为时请显式设置 ttl

示例：

```toml
[[channels]]
id = "claude"
enabled = true

[channels.settings]
base_url = "https://api.anthropic.com"
cache_breakpoints = [
  { target = "top_level", ttl = "auto" },
  { target = "messages", position = "last_nth", index = 1, ttl = "5m" }
]

[[channels]]
id = "claudecode"
enabled = true

[channels.settings]
base_url = "https://api.anthropic.com"
cache_breakpoints = [
  { target = "top_level", ttl = "auto" },
  { target = "messages", position = "last_nth", index = 1, ttl = "1h" }
]
```

### `channels.credentials`

每个凭证可包含：

- `id` / `label`: 可选标识
- `secret`: API Key 场景使用
- `builtin`: OAuth/ServiceAccount 场景使用结构化对象
- `state`: 可选健康状态种子

`state.health.kind` 支持：

- `healthy`
- `partial`（带 `models` 冷却列表）
- `dead`

### 凭证选择模式与缓存亲和池

Provider 的凭证选择由 `channels.settings` 里的两个开关控制：

- `credential_round_robin_enabled`（默认 `true`）
- `credential_cache_affinity_enabled`（默认 `true`，仅在启用轮询时生效）

最终行为：

- `credential_round_robin_enabled = false` -> `StickyNoCache`
  - 不轮询
  - 不启用缓存亲和池
  - 固定选择当前可用凭证中 `id` 最小的一个；该凭证冷却/不可用时再切换
- `credential_round_robin_enabled = true` 且 `credential_cache_affinity_enabled = true` -> `RoundRobinWithCache`
  - 在可用凭证中轮询/随机
  - 启用内部缓存亲和池，尽量把同一缓存键路由到同一凭证
- `credential_round_robin_enabled = true` 且 `credential_cache_affinity_enabled = false` -> `RoundRobinNoCache`
  - 在可用凭证中轮询/随机
  - 不启用缓存亲和池

示例：

```toml
[[channels]]
id = "openai"
enabled = true

[channels.settings]
base_url = "https://api.openai.com"
credential_round_robin_enabled = true
credential_cache_affinity_enabled = true
```

兼容性说明：

- 仍兼容旧字段 `credential_pick_mode`。

完整设计与上游缓存命中策略（OpenAI/Claude/Gemini）详见：  
https://gproxy.leenhawk.com/zh/guides/credential-selection-cache-affinity/

## CLI 与环境变量覆盖

配置优先级：`CLI 参数 / 环境变量 > gproxy.toml > 默认值`

支持覆盖项：

- `--config` / `GPROXY_CONFIG_PATH`
- `--host` / `GPROXY_HOST`
- `--port` / `GPROXY_PORT`
- `--proxy` / `GPROXY_PROXY`
- `--admin-key` / `GPROXY_ADMIN_KEY`
- `--bootstrap-force-config` / `GPROXY_BOOTSTRAP_FORCE_CONFIG`
- `--mask-sensitive-info` / `GPROXY_MASK_SENSITIVE_INFO`
- `--data-dir` / `GPROXY_DATA_DIR`
- `--dsn` / `GPROXY_DSN`
- `--storage-write-queue-capacity` / `GPROXY_STORAGE_WRITE_QUEUE_CAPACITY`
- `--storage-write-max-batch-size` / `GPROXY_STORAGE_WRITE_MAX_BATCH_SIZE`
- `--storage-write-aggregate-window-ms` / `GPROXY_STORAGE_WRITE_AGGREGATE_WINDOW_MS`

### 启动数据来源模式

`--bootstrap-force-config` / `GPROXY_BOOTSTRAP_FORCE_CONFIG` 是启动期开关（仅 CLI/ENV，非 `gproxy.toml` 字段）。

- 默认（`false` 或未设置）：
  - 若数据库未初始化，按 `gproxy.toml` 正常引导；
  - 若数据库已初始化，优先使用数据库状态，并跳过配置文件中的渠道/provider 导入；
  - 启动时传入的 `admin_key` 覆盖仍然生效。
- `true`：
  - 启动时强制应用配置文件中的 channels/settings/credentials/global；
  - 适用于你明确希望用配置文件覆盖已存在数据库引导状态的场景。

## API 概览

所有错误统一返回：

```json
{ "error": "..." }
```

### 认证头

- `POST /login` 使用 JSON 请求体 `{ "name": "...", "password": "..." }`，返回 `api_key`
- 管理/用户接口（除 `/login` 外）：使用 `x-api-key`
- Provider 接口支持：
  - `x-api-key`
  - `x-goog-api-key`
  - `Authorization: Bearer ...`
  - Gemini 场景 query `?key=...`（会被归一化到 `x-api-key`）

### Provider 路由

#### 1) Scoped（推荐）

带 provider 前缀路径，示例：

- `POST /openai/v1/chat/completions`
- `POST /claude/v1/messages`
- `POST /aistudio/v1beta/models/{model}:generateContent`

#### 2) Unscoped（统一入口）

不带 provider 前缀路径，通道由 `model` 前缀决定：

- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/messages`
- `GET /v1/models`
- `GET /v1/models/{provider}/{model}`

约束：

- OpenAI/Claude 风格 body 的 `model` 必须是 `<provider>/<model>`，如 `openai/gpt-4.1`
- Gemini path 风格 target 需带 provider 前缀，如 `models/aistudio/gemini-2.5-flash:generateContent`

### OAuth 与上游 usage

- `GET /{provider}/v1/oauth`
- `GET /{provider}/v1/oauth/callback`
- `GET /{provider}/v1/usage`

支持 OAuth 的通道：`codex`、`claudecode`、`geminicli`、`antigravity`

### 管理接口（`/admin/*`）

主要分组：

- 全局设置：`/admin/global-settings`、`/admin/global-settings/upsert`
- 配置导入导出：`/admin/config/export-toml`、`/admin/config/import-toml`
- 自更新：`/admin/system/self_update`
- Providers/Credentials/CredentialStatuses：`query/upsert/delete`
- Users：`query/upsert/delete`（`/admin/users/upsert` 需要 `password`）
- UserKeys：`query/generate/delete`
- Requests：`/admin/requests/upstream/query`、`/admin/requests/downstream/query`
- Usage：`/admin/usages/query`、`/admin/usages/summary`

### 用户接口（`/user/*`）

- `POST /user/keys/query`
- `POST /user/keys/generate`
- `POST /user/keys/delete`
- `POST /user/usages/query`
- `POST /user/usages/summary`

## 请求示例

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

### Unscoped OpenAI Chat（model 前缀路由）

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

### Claude/ClaudeCode Prompt Cache 快速验证（4 条 curl）

先确认这两个 provider 至少配置了一条 `cache_breakpoints`（例如 `{ target = "top_level", ttl = "auto" }`）。

```bash
BASE="http://127.0.0.1:8787"
KEY="<你的 x-api-key>"
SYS="$(for i in $(seq 1 1800); do printf 'cache-prefix-%04d ' "$i"; done)"
```

```bash
# 1) Claude 第一次请求（写缓存）
curl -sS "$BASE/claude/v1/messages" \
  -H "x-api-key: $KEY" \
  -H "content-type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  --data-binary @- <<JSON | jq '.usage'
{
  "model": "claude-neptune-v3",
  "max_tokens": 128,
  "stream": false,
  "system": "$SYS",
  "messages": [
    { "role": "user", "content": "只回复: cache-ok" }
  ]
}
JSON
```

```bash
# 2) Claude 第二次请求（读缓存）
curl -sS "$BASE/claude/v1/messages" \
  -H "x-api-key: $KEY" \
  -H "content-type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  --data-binary @- <<JSON | jq '.usage'
{
  "model": "claude-neptune-v3",
  "max_tokens": 128,
  "stream": false,
  "system": "$SYS",
  "messages": [
    { "role": "user", "content": "只回复: cache-ok" }
  ]
}
JSON
```

```bash
# 3) ClaudeCode 第一次请求（写缓存）
curl -sS "$BASE/claudecode/v1/messages" \
  -H "x-api-key: $KEY" \
  -H "content-type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  --data-binary @- <<JSON | jq '.usage'
{
  "model": "claude-sonnet-4-6",
  "max_tokens": 128,
  "stream": false,
  "system": "$SYS",
  "messages": [
    { "role": "user", "content": "只回复: cache-ok" }
  ]
}
JSON
```

```bash
# 4) ClaudeCode 第二次请求（读缓存）
curl -sS "$BASE/claudecode/v1/messages" \
  -H "x-api-key: $KEY" \
  -H "content-type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  --data-binary @- <<JSON | jq '.usage'
{
  "model": "claude-sonnet-4-6",
  "max_tokens": 128,
  "stream": false,
  "system": "$SYS",
  "messages": [
    { "role": "user", "content": "只回复: cache-ok" }
  ]
}
JSON
```

## 架构总览

### Workspace 结构

| 路径 | 作用 |
|---|---|
| `apps/gproxy` | 可执行服务入口（Axum + 管理前端静态资源） |
| `crates/gproxy-core` | AppState、路由编排、鉴权与请求执行 |
| `crates/gproxy-provider` | 通道实现、重试、OAuth、dispatch、tokenizer |
| `crates/gproxy-middleware` | 协议变换中间件、usage 提取 |
| `crates/gproxy-protocol` | OpenAI/Claude/Gemini 类型与转换模型 |
| `crates/gproxy-storage` | SeaORM 存储层、查询模型、异步写队列 |
| `crates/gproxy-admin` | 管理域逻辑（admin/user） |

### 运行机制

- 启动时：
  - 加载配置并应用 CLI/ENV 覆盖
  - 依据 `bootstrap_force_config` 选择引导来源：数据库初始化后默认优先数据库
  - 建立数据库连接并自动 schema sync
  - 初始化 provider registry、凭证与状态
  - 确保 admin 用户（`id=0`）和 admin key 存在
- 请求时：
  - 用户 key 鉴权
  - 依据路由 + dispatch 进行协议转换/透传
  - 从可用凭证中随机选择并重试回退
  - 记录 upstream/downstream request 与 usage

### 凭证状态与冷却

- `healthy`: 可用
- `partial`: 指定模型冷却（模型级）
- `dead`: 凭证不可用

默认冷却时间：

- rate limit：`60s`
- transient failure：`15s`

## 测试

仓库提供了 provider smoke/regression 脚本：

- `tests/provider/curl_provider.sh`
- `tests/provider/run_channel_regression.sh`

示例：

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

## 常见问题

### 1) `401 unauthorized`

- 对需要 key 的接口，检查 `x-api-key` 是否存在且对应用户/密钥均为 enabled。
- 如果还没有 key，先用用户名密码调用 `POST /login` 获取。

### 2) `403 forbidden`（admin 路由）

- 你使用的 key 不是 admin 用户（`id=0`）的 key。

### 3) `503 all eligible credentials exhausted`

- 检查：
  - 通道下是否有可用凭证
  - `credential_status` 是否被标记为 `dead` 或模型处于 `partial` 冷却
  - 上游是否在持续 429/5xx

### 4) `model must be prefixed as <provider>/...`

- 你调用的是 unscoped 路由，`model` 没写 provider 前缀。

### 5) 实时 WebSocket 不可用

- `/v1/realtime` 目前返回“未实现”，请使用 `/v1/responses`（HTTP）路径。

## 安全建议

- 生产环境务必设置强 `admin_key`，避免使用默认自动生成后长期不变更。
- 建议保持 `mask_sensitive_info = true`，避免请求体/响应体明文落盘。
- 若启用上游代理，确认代理链路可信且具备访问控制。

## 数据与目录

默认情况下：

- 数据目录：`./data`
- 默认数据库：`sqlite://./data/gproxy.db?mode=rwc`
- tokenizer 缓存目录：`./data/tokenizers`

`gproxy-storage` 同时支持 sqlite / mysql / postgres（通过 `dsn` 选择）。

## 开发命令

```bash
# 后端格式化/检查
cargo fmt
cargo check
cargo clippy --workspace --all-targets

# 测试
cargo test --workspace

# 启动服务
cargo run -p gproxy
```

前端：

```bash
cd apps/gproxy/frontend
pnpm install
pnpm typecheck
pnpm build
```

## License

本项目采用 `AGPL-3.0-or-later`（见 `LICENSE`）。

## Star 趋势

[![Star History Chart](https://api.star-history.com/svg?repos=LeenHawk/gproxy&type=Date)](https://star-history.com/#LeenHawk/gproxy&Date)
