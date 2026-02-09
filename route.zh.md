# GProxy 路由（当前）

## 代理（Proxy）

### 鉴权（下游）
支持的用户密钥来源（按顺序匹配，命中即用）：
- `Authorization: Bearer <key>`
- `x-api-key: <key>`
- `x-goog-api-key: <key>`
- 查询参数 `?key=<key>`

注意：下游鉴权头/查询参数在转发到上游前会被**剥离**。  
注意：`trace_id` 由服务端按每个下游请求生成（UUIDv7），并复用于对应的全部上游事件；请求头 `x-trace-id` / `x-request-id` 会被忽略。

### 聚合路由（`/...`，不带 `{provider}`）

#### Claude
- `POST /v1/messages`
- `POST /v1/messages/count_tokens`

#### OpenAI
- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/responses/input_tokens`

#### 共享模型路由
- `GET /v1/models`
- `GET /v1/models/{model}`

路由判定：`GET /v1/models` + `GET /v1/models/{model}`：
- 当存在 `anthropic-version` 头时，按 Claude 处理。
- 当下游 key 形态为 Gemini（`x-goog-api-key` 或 `?key=`）时，按 Gemini v1 处理。
- 其余情况按 OpenAI 处理。

#### Gemini
- `POST /v1/models/{model}:generateContent`
- `POST /v1/models/{model}:streamGenerateContent`
- `POST /v1/models/{model}:countTokens`
- `POST /v1beta/models/{model}:generateContent`
- `POST /v1beta/models/{model}:streamGenerateContent`
- `POST /v1beta/models/{model}:countTokens`
- `GET /v1beta/models`
- `GET /v1beta/models/{name}`

#### 模型前缀规则（`provider/model`）
- 聚合请求中的模型标识必须使用 `provider/model`。
- 拆分规则只按第一个 `/` 分割，所以模型名本身仍可包含 `/`。
- 缺失或非法前缀会返回 `400`，并带 `error=missing_provider_prefix`。

#### 聚合模型列表响应扩展
对 `GET /v1/models` 与 `GET /v1beta/models`，响应会包含：
- `partial: boolean`

错误处理策略：
- `no_active_credentials` 会被静默跳过。
- `unsupported_operation` / `provider_disabled` 会被静默跳过。
- 其他 provider 失败仅会体现为 `partial=true`；不会向下游返回详细错误。
- HTTP 状态码始终为 `200`。

#### 响应模型名规范化
仅在聚合路由中，响应里的模型标识会规范化为带 provider 前缀：
- OpenAI/Claude 模型字段：`provider/model`
- Gemini 模型资源名：`models/provider/model`

已有的 provider 前缀路由（`/{provider}/...`）保持不变。

### Provider 路由（`/{provider}/...`）

### Claude
- `POST /{provider}/v1/messages`
- `POST /{provider}/v1/messages/count_tokens`
- `GET /{provider}/v1/models`
- `GET /{provider}/v1/models/{model}`

路由判定：当存在 `anthropic-version` 头时，`GET /v1/models` + `GET /v1/models/{model}` 会按 **Claude** 处理。

### OpenAI
- `POST /{provider}/v1/chat/completions`
- `POST /{provider}/v1/responses`
- `POST /{provider}/v1/responses/input_tokens`
- `GET /{provider}/v1/models`
- `GET /{provider}/v1/models/{model}`

路由判定：`GET /v1/models` + `GET /v1/models/{model}` 在不属于 Claude/Gemini 时默认按 **OpenAI** 处理。

### Gemini
#### 生成 / 流式 / 计数（v1 与 v1beta）
- `POST /{provider}/v1/models/{model}:generateContent`
- `POST /{provider}/v1/models/{model}:streamGenerateContent`
- `POST /{provider}/v1/models/{model}:countTokens`

- `POST /{provider}/v1beta/models/{model}:generateContent`
- `POST /{provider}/v1beta/models/{model}:streamGenerateContent`
- `POST /{provider}/v1beta/models/{model}:countTokens`

#### 模型
- `GET /{provider}/v1beta/models`
- `GET /{provider}/v1beta/models/{name}`

在 `GET /v1/models` + `GET /v1/models/{model}` 上的路由判定：
- 当下游 key 为 **Gemini 形态**（`x-goog-api-key` 或 `?key=`）时，按 **Gemini v1** 处理。

### Provider 内部下游能力
- `GET /{provider}/oauth`
- `GET /{provider}/oauth/callback`
- `GET /{provider}/usage`
  - 必填查询参数：`credential_id=<id>`。
  - Usage 会针对该 provider 下这个指定 credential 拉取。

#### OAuth 行为说明

##### codex（`device` 流程）
- `GET /codex/oauth`
  - 启动对 OpenAI 的 Device Authorization 流程。
  - 返回：`mode=device`, `auth_url`, `verification_uri`, `user_code`, `interval`, `state`。
- `GET /codex/oauth/callback`
  - 通过 `state` 继续流程（不需要 `code`）。
  - 轮询 device authorization 状态，并使用 `redirect_uri=https://auth.openai.com/deviceauth/callback` 换 token。
  - 若用户尚未完成授权，返回 `409`，并带 `error=authorization_pending: retry after <n>s`。

##### claudecode（`manual` 流程）
- `GET /claudecode/oauth`
  - 返回手动授权信息：`mode=manual`, `auth_url`, `state`, `redirect_uri`。
  - 默认回调：`https://platform.claude.com/oauth/code/callback`。
- `GET /claudecode/oauth/callback`
  - 接受 `?code=...` 或 `?callback_url=...`（手动传入的 `code` 优先）。

##### geminicli（`manual` 流程）
- `GET /geminicli/oauth`
  - 返回手动授权信息：`mode=manual`, `auth_url`, `state`, `redirect_uri`。
  - 默认回调：`https://codeassist.google.com/authcode`。
- `GET /geminicli/oauth/callback`
  - 接受 `?code=...` 或 `?callback_url=...`（手动传入的 `code` 优先）。

##### antigravity（`manual` 流程）
- `GET /antigravity/oauth`
  - 返回手动授权信息：`mode=manual`, `auth_url`, `state`, `redirect_uri`。
  - 默认回调：`http://localhost:51121/oauth-callback`。
- `GET /antigravity/oauth/callback`
  - 接受 `?code=...` 或 `?callback_url=...`（手动传入的 `code` 优先）。

##### state 解析规则
- 显式传入 `state` 时，优先使用该值。
- 若未传 `state` 且当前仅存在一个待处理 OAuth state，则自动使用它。
- 若未传 `state` 且存在多个待处理 state，则 callback 返回 `400`，并带 `error=ambiguous_state`。


## 管理端（/admin/...）

### 鉴权（admin）
支持的管理员密钥来源（按顺序匹配，命中即用）：
- `x-admin-key: <key>`
- `Authorization: Bearer <key>`

### 路由
- `GET /admin/health`
- `GET /admin/global_config`
- `PUT /admin/global_config`

- `GET /admin/providers`
- `GET /admin/providers/{name}`
- `PUT /admin/providers/{name}`
- `DELETE /admin/providers/{name}`（仅 custom 可删；内置 provider 需通过禁用处理）

- `GET /admin/providers/{name}/credentials`
- `POST /admin/providers/{name}/credentials`

- `GET /admin/credentials`
- `PUT /admin/credentials/{id}`
- `DELETE /admin/credentials/{id}`
- `PUT /admin/credentials/{id}/enabled`

- `GET /admin/usage/providers/{provider}/tokens?from=<RFC3339>&to=<RFC3339>`
- `GET /admin/usage/providers/{provider}/models/{model}/tokens?from=<RFC3339>&to=<RFC3339>`
- `GET /admin/usage/credentials/{credential_id}/tokens?from=<RFC3339>&to=<RFC3339>`
- `GET /admin/usage/credentials/{credential_id}/models/{model}/tokens?from=<RFC3339>&to=<RFC3339>`

- `GET /admin/users`
- `PUT /admin/users/{id}`
- `DELETE /admin/users/{id}`
- `PUT /admin/users/{id}/enabled`

- `GET /admin/users/{id}/keys`
- `POST /admin/users/{id}/keys`
- `PUT /admin/user_keys/{id}`
- `DELETE /admin/user_keys/{id}`
- `PUT /admin/user_keys/{id}/enabled`

- `GET /admin/logs`

注意：usage 记录持久化在 DB 表 `upstream_usages`（不是 `upstream_requests.usage_json`）。  
注意：`upstream_usages` 包含 `model` 列；模型维度 usage 路由按该列过滤。  
注意：历史数据在请求体/路径未含模型信息，或 `event_redact_sensitive=true`（请求体未持久化，无法提取/回填模型）时，`model` 可能为 `NULL`。
注意：`GET /admin/logs` 使用游标分页（`cursor_at` + `cursor_id`），`offset>0` 会被拒绝以避免性能问题。  
注意：`GET /admin/logs` 默认 `include_body=false`，除非显式开启，否则不会返回请求/响应 body。
