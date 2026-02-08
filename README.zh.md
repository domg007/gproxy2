# gproxy

[English](README.MD) | [简体中文](README.zh.md)

一个用 Rust 编写的高性能多渠道 LLM 网关，内嵌管理 SPA。

gproxy 提供：
- 统一下游 API（OpenAI / Claude / Gemini 风格路由）
- 渠道与凭证级路由能力
- 支持渠道的 OAuth 辅助流程
- Usage 聚合统计 + 部分渠道实时用量查询
- 内置管理 API + React 19 + Tailwind 4 管理界面（`/`）

## 一键部署

[![Deploy on Zeabur](https://zeabur.com/button.svg)](https://zeabur.com/templates/68HULI)
[![Deploy to Render](https://render.com/images/deploy-to-render-button.svg)](https://render.com/deploy?repo=https://github.com/LeenHawk/gproxy)

- Zeabur 模板文件：`zeabur.yaml`
- Render Blueprint 文件：`render.yaml`
- Render Blueprint 默认不创建托管 PostgreSQL；`GPROXY_DSN` 保留为可选项，便于接入外部数据库。
- Render Blueprint 默认不挂载持久磁盘；`GPROXY_DATA_DIR` 默认为临时目录（`/tmp/gproxy-data`），如需持久化请使用外部存储。

## 内置渠道

首次启动会自动写入以下内置渠道：

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

你也可以在管理界面/API 中新增 `custom` 类型渠道。

## 工程结构（workspace）

- `apps/gproxy`：可运行服务（二进制，包含 proxy + admin API + 内嵌前端）
- `crates/gproxy-core`：启动引导、内存状态、代理引擎
- `crates/gproxy-router`：HTTP 路由（`/` 与 `/admin`）
- `crates/gproxy-provider-core`：渠道抽象、配置、凭证/运行时状态
- `crates/gproxy-provider-impl`：内置渠道实现
- `crates/gproxy-storage`：SeaORM 存储与 usage 持久化
- `apps/gproxy/frontend`：管理前端源码（React 19 + Tailwind 4）

## 本地快速启动

前置条件：
- Rust stable
- Node.js + pnpm（仅在需要重建前端资源时需要）

1. 构建管理前端资源

```bash
pnpm -C apps/gproxy/frontend install --frozen-lockfile
pnpm -C apps/gproxy/frontend build
```

2. 启动服务

```bash
cargo run -p gproxy -- --admin-key your-admin-key
```

3. 打开管理界面

- 管理端：`http://127.0.0.1:8787/`

默认监听 `0.0.0.0:8787`，可通过 CLI/环境变量/DB 合并配置覆盖。

## 配置说明

启动时全局配置合并顺序：`CLI > ENV > DB`，合并结果会回写数据库。

CLI / ENV（来自 `gproxy_core::bootstrap::CliArgs`）：

- `--dsn` / `GPROXY_DSN`（默认：`sqlite://gproxy.db?mode=rwc`）
- `--host` / `GPROXY_HOST`（合并后默认：`0.0.0.0`）
- `--port` / `GPROXY_PORT`（合并后默认：`8787`）
- `--admin-key` / `GPROXY_ADMIN_KEY`（明文输入，存储时会 hash）
- `--proxy` / `GPROXY_PROXY`（可选，上游出口代理）
- `--event-redact-sensitive` / `GPROXY_EVENT_REDACT_SENSITIVE`（默认：`true`）

说明：
- 若未提供 `admin_key` 且 DB 中也不存在，启动时会自动生成并打印一次。
- 若缺失内置渠道，会在启动时自动补种子。
- 对文件型 SQLite DSN，gproxy 启动时会自动创建缺失的父目录；当使用 `mode=rwc` 时，数据库文件不存在也会自动创建。

### `custom` 渠道 JSON 参数屏蔽

`custom` 渠道支持 `channel_settings.json_param_mask`，可在请求发往上游前，将指定 JSON 字段强制置为 `null`。

- 仅对 JSON 请求体生效（`content-type: application/json`）
- 非 JSON 请求不受影响
- 路径不存在时会忽略，不报错

支持的路径写法（每行/每项一个）：

- 顶层字段：`temperature`
- 点路径/索引：`messages[1].content`
- 通配符：`messages[*].content`
- JSON Pointer：`/messages/0/content`

示例：

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

## 认证模型

### 管理端（`/admin/...`）

支持以下 admin key 来源（按顺序匹配）：
- `x-admin-key: <key>`
- `Authorization: Bearer <key>`
- Query `?admin_key=<key>`（浏览器连接 `/admin/events/ws` 时有用）

### 代理下游（`/v1/...` 或 `/{provider}/...`）

支持以下 user key 来源（按顺序匹配）：
- `Authorization: Bearer <key>`
- `x-api-key: <key>`
- `x-goog-api-key: <key>`
- Query `?key=<key>`

启动时会自动创建 `user0`，并插入一条与 admin key 同 hash 的 user key，因此早期测试时可直接用同一明文 key 访问 proxy。

## API 概览

完整路由请看 [`route.md`](route.zh.md)。

主要分组：
- 无渠道前缀的聚合代理路由（如 `/v1/chat/completions`、`/v1/models`）
- 带渠道前缀的代理路由（如 `/openai/v1/chat/completions`）
- 渠道内部能力路由：
  - `GET /{provider}/oauth`
  - `GET /{provider}/oauth/callback`
  - `GET /{provider}/usage?credential_id=<id>`
- 管理路由 `/admin/...`（渠道、凭证、用户、usage、事件流）

## 管理前端

管理 SPA 挂载在 `/`，静态资源在 `/assets/*`。

当前前端模块包括：
- 渠道配置（含 `custom` 渠道编辑）
- 凭证管理（查看/编辑/删除/启停，运行时状态）
- 批量凭证导入（key/json）
- OAuth 助手（支持的渠道）
- 凭证级实时用量/额度视图
- 用户与 API key 管理
- 终端事件流查看（`/admin/events/ws`）
- 多语言（`zh_cn` / `en`）

## 构建与发布

### 二进制

```bash
cargo build --release -p gproxy
```

### Docker 镜像

构建：

```bash
docker build -t gproxy:local .
```

运行：

```bash
docker run --rm -p 8787:8787 \
  -e GPROXY_HOST=0.0.0.0 \
  -e GPROXY_PORT=8787 \
  -e GPROXY_ADMIN_KEY=your-admin-key \
  -e GPROXY_DSN='sqlite://app/data/gproxy.db?mode=rwc' \
  -v $(pwd)/data:/app/data \
  gproxy:local
```

### GitHub Actions

- `.github/workflows/docker.yml`：构建并推送多架构 GHCR 镜像
- `.github/workflows/release-binary.yml`：跨 OS/arch 构建发布二进制

## 相关文档

- `route.md`：路由与行为说明
- `provider.md`：各渠道凭证/配置细节
- `PLAN.md`：项目计划草案

## License

AGPL-3.0-or-later
