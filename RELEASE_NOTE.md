# Release Notes

## v0.3.41

### English

#### Added

- Added true request-log deletion actions in the admin Requests workspace. `Delete Selected Logs` and `Delete All Logs` now remove upstream/downstream request rows entirely instead of only clearing stored headers/bodies.

#### Changed

- ClaudeCode now always ensures the Claude Code `x-anthropic-billing-header` system block is present upstream when missing. The old channel-level `claudecode_enable_billing_header` toggle has been removed from both provider settings and the admin UI.
- If a ClaudeCode request already contains an `x-anthropic-billing-header` system block, gproxy now preserves that existing block instead of recomputing or overwriting it.
- The admin Requests workspace now separates `Clear Payload` from `Delete Logs` so payload wiping and row deletion are no longer conflated in the UI.

#### Fixed

- Fixed ClaudeCode bulk credential import/export mismatch in the admin workspace. Copying a ClaudeCode credential and pasting it back into bulk import now round-trips as JSON correctly instead of being misread as a cookie-only line import.

#### Compatibility

- No storage migration is required.
- Existing provider settings that still contain `claudecode_enable_billing_header` remain readable; the flag is now ignored because billing-header insertion is default behavior.
- Request-log deletion is permanent and removes the full stored row, unlike payload clearing which still keeps the row metadata.

### 中文

#### 新增

- 后台 Requests 工作区新增真正的请求日志删除操作。`删除选中日志` 和 `删除全部日志` 现在会直接删除 upstream/downstream 请求记录整行，而不再只是清空已存储的 header/body。

#### 变更

- ClaudeCode 现在默认会在上游请求中确保存在 Claude Code 的 `x-anthropic-billing-header` system block；原有渠道级 `claudecode_enable_billing_header` 开关已从 provider settings 和后台 UI 中移除。
- 如果 ClaudeCode 请求本身已经带有 `x-anthropic-billing-header` system block，gproxy 现在会保留原值，不再重新计算或覆盖。
- 后台 Requests 工作区现已将 `清空载荷` 与 `删除日志` 两类操作分开展示，避免 UI 上把 payload 清理和整行删除混为一谈。

#### 修复

- 修复后台 ClaudeCode 凭证批量导入/导出格式不一致的问题。现在从卡片复制 ClaudeCode 凭证后，直接粘贴回批量导入框可以按 JSON 正确回导，不会再被误判成仅 cookie 的单行导入。

#### 兼容性

- 无需执行存储迁移。
- 旧 provider settings 中如果仍包含 `claudecode_enable_billing_header`，仍可正常读取；该字段现在会被忽略，因为 billing header 注入已成为默认行为。
- 请求日志删除为永久删除，会移除完整记录；若只想保留元信息，应继续使用 `清空载荷`。

## v0.3.40

### English

#### Fixed

- OAuth credential persistence now distinguishes same-email Codex logins by `account_id` and same-email ClaudeCode logins by `organization_uuid`, preventing different workspaces/subscriptions from collapsing into a single stored credential. ClaudeCode OAuth profile parsing also records `organization_uuid` and prefers the upstream `organization_type` when available.

#### Compatibility

- No storage migration is required.

### 中文

#### 修复

- OAuth 凭证持久化现在会用 `account_id` 区分同邮箱的 Codex 登录，并用 `organization_uuid` 区分同邮箱的 ClaudeCode 登录，避免不同 workspace / 订阅被折叠成同一条凭证。ClaudeCode 的 OAuth profile 解析也会记录 `organization_uuid`，并在上游返回时优先采用 `organization_type` 作为订阅类型。

#### 兼容性

- 无需执行存储迁移。

## v0.3.39

### English

#### Added

- Added an optional channel-level `claudecode_enable_billing_header` setting for ClaudeCode. When enabled, gproxy injects the Claude Code `x-anthropic-billing-header` as the first upstream `system` block and computes the `cc_version` hash suffix dynamically from the first user message, matching Claude Code `2.1.76` behavior.

#### Changed

- Updated the built-in ClaudeCode user-agent defaults/templates to `claude-code/2.1.76` and `claude-cli/2.1.76 (external, cli)`.
- ClaudeCode billing-header insertion now runs after upstream `cache_control` rewrite and cache-breakpoint processing, ensuring the injected billing `system` block never receives `cache_control`.

#### Compatibility

- No storage migration is required.
- Existing ClaudeCode providers remain unchanged unless `claudecode_enable_billing_header` is explicitly enabled.
- Existing cache-breakpoint behavior remains unchanged for user-defined payload blocks; only the injected billing header is kept free of `cache_control`.

### 中文

#### 新增

- ClaudeCode 渠道新增可选的 `claudecode_enable_billing_header` 设置。启用后，gproxy 会在上游请求中把 Claude Code 的 `x-anthropic-billing-header` 作为第一个 `system` block 注入，并按 Claude Code `2.1.76` 规则基于首条 user 消息动态计算 `cc_version` 的 hash 后缀。

#### 变更

- 内置 ClaudeCode 的 User-Agent 默认值 / 模板已更新为 `claude-code/2.1.76` 和 `claude-cli/2.1.76 (external, cli)`。
- ClaudeCode billing header 的注入时机已调整到上游 `cache_control` 改写和 cache breakpoint 处理之后，确保该 billing `system` block 不会被附加 `cache_control`。

#### 兼容性

- 无需执行存储迁移。
- 现有 ClaudeCode provider 默认行为不变；只有显式开启 `claudecode_enable_billing_header` 后才会注入该 billing header。
- 现有 cache breakpoint 对用户自定义 payload block 的行为保持不变；仅新增的 billing header 会被明确保持为不带 `cache_control`。

## v0.3.38

### English

#### Added

- Added per-credential `enable_claude_1m_sonnet` / `enable_claude_1m_opus` fields for ClaudeCode credentials in the admin credential workspace. Newly created ClaudeCode OAuth credentials initialize both flags as enabled.
- Added the Anthropic `compact-2026-01-12` beta tag to the built-in ClaudeCode beta reference list / known beta values so it can be selected and normalized like other beta headers.

#### Fixed

- ClaudeCode requests now preserve `context_management` payloads instead of stripping them, allowing compact-mode related context edits to pass through upstream when requested.
- ClaudeCode `anthropic-beta` request headers now accept both array values and a single comma-separated string, while still normalizing duplicates and keeping the required OAuth beta.
- ClaudeCode `context-1m-*` beta forwarding is now guarded per credential and per model family again. If a request fails with `context-1m` enabled, the proxy retries once without that beta; when the retry succeeds, it automatically disables the corresponding 1M flag for that credential to avoid repeated failures.
- Transport-level send failures such as `error sending request for uri ... client error (SendRequest)` no longer add cooldowns or downgrade credential health; the existing health state is preserved.

#### Compatibility

- No storage migration is required.
- Existing ClaudeCode credentials that do not yet contain `enable_claude_1m_sonnet` / `enable_claude_1m_opus` remain compatible and default to enabled behavior until a successful fallback retry disables the matching flag.
- Existing array-based `anthropic-beta` payloads continue to work unchanged; flat string values are now accepted as an additional compatible form.

### 中文

#### 新增

- ClaudeCode 凭证在后台凭证工作区新增按凭证控制的 `enable_claude_1m_sonnet` / `enable_claude_1m_opus` 字段。新创建的 ClaudeCode OAuth 凭证会默认将这两个开关初始化为启用。
- 内置 ClaudeCode beta 参考列表 / 已知 beta 值新增 Anthropic `compact-2026-01-12`，现在可像其他 beta 头一样直接选择并参与规范化。

#### 修复

- ClaudeCode 请求不再剥离 `context_management` 字段，启用 compact mode 等上下文编辑能力时，可按请求原样透传到上游。
- ClaudeCode 的 `anthropic-beta` 请求头现在同时支持数组形式和单个逗号分隔字符串形式，并继续保留去重与自动补齐 OAuth beta 的行为。
- ClaudeCode 的 `context-1m-*` beta 转发恢复为按凭证、按模型族控制。如果请求在启用 `context-1m` 时失败，代理会自动去掉该 beta 重试一次；若重试成功，会自动关闭该凭证对应的 1M 开关，避免后续重复失败。
- `error sending request for uri ... client error (SendRequest)` 这类传输层发送失败不再给凭证追加 cooldown，也不会下调凭证健康状态；会保留当前健康状态，仅记录最新错误信息。

#### 兼容性

- 无需执行存储迁移。
- 尚未包含 `enable_claude_1m_sonnet` / `enable_claude_1m_opus` 字段的旧 ClaudeCode 凭证保持兼容；在未触发成功降级回退前，默认仍按启用状态处理。
- 现有数组形式的 `anthropic-beta` 载荷无需调整；现在只是额外兼容单个逗号分隔字符串形式。

## v0.3.37

### English

#### Changed

- Removed per-credential `enable_claude_1m_sonnet` / `enable_claude_1m_opus` permission fields from ClaudeCode credentials. All ClaudeCode credentials now unconditionally support 1M context — no per-account capability check is performed.
- Removed model-based automatic `context-1m-2025-08-07` beta header injection and filtering. Whether the `context-1m` beta is sent upstream is now controlled entirely by request headers or the channel-level `claudecode_extra_beta_headers` setting, consistent with all other beta headers.
- Removed the automatic fallback retry logic that stripped `context-1m` and retried on upstream failure, along with the mechanism that disabled 1M for a credential after such a failure.

#### Compatibility

- No storage migration is required.
- Existing credentials that still contain `enable_claude_1m_sonnet` / `enable_claude_1m_opus` in storage will deserialize without error (the fields are simply ignored).
- If you previously relied on the proxy automatically injecting `context-1m-2025-08-07` for supported models, you should now add it to `claudecode_extra_beta_headers` in your channel settings to preserve the same behavior.

### 中文

#### 变更

- 移除 ClaudeCode 凭证中的 `enable_claude_1m_sonnet` / `enable_claude_1m_opus` 权限字段。所有 ClaudeCode 凭证现在无条件支持 1M 上下文，不再执行按账号的能力检查。
- 移除根据目标模型自动注入或过滤 `context-1m-2025-08-07` beta 头的逻辑。是否发送 `context-1m` beta 现完全由请求头或渠道级 `claudecode_extra_beta_headers` 设置控制，与其他 beta 头行为一致。
- 移除上游失败后自动剥离 `context-1m` 并重试的降级逻辑，以及失败后自动为凭证禁用 1M 的机制。

#### 兼容性

- 无需执行存储迁移。
- 存储中仍包含 `enable_claude_1m_sonnet` / `enable_claude_1m_opus` 的旧凭证可正常反序列化（字段会被忽略）。
- 如果此前依赖代理为支持的模型自动注入 `context-1m-2025-08-07`，现在需要在渠道设置的 `claudecode_extra_beta_headers` 中手动添加该值以保持相同行为。

## v0.3.36

### English

#### Added

- Added message-scoped content selectors for built-in Claude `cache_breakpoints`. `messages` rules can now optionally use `content_position` / `content_index` to select a block inside the matched message, and the admin provider config now exposes the same selector mode.

#### Changed

- Cache breakpoint matching now normalizes Claude shorthand content first, so forms like `system: "..."` and `messages[*].content: "..."` are indexed the same way as canonical text blocks.
- Built-in Claude channels now treat no-TTL ephemeral `cache_control` consistently as `5m` when deriving cache-affinity behavior and examples.

#### Fixed

- Fixed Codex credential health detection: upstream `402 deactivated_workspace` responses are now recognized as dead credentials for both regular upstream requests and `v1/usage`, instead of being retried as transient failures.
- Fixed OpenAI `responses -> chat completions` stream decoding when `response.function_call_arguments.done` omits `name`, preventing stream errors such as `failed to decode response_stream_json ... missing field 'name'`.

#### Compatibility

- No storage migration is required.
- Existing Claude `cache_breakpoints` settings remain valid; `content_position` / `content_index` are optional enhancements.
- Codex credentials may now transition to `dead` automatically when upstream explicitly returns `402 deactivated_workspace`.

### 中文

#### 新增

- built-in Claude 渠道的 `cache_breakpoints` 新增 message 内 block 选择能力。`messages` 规则现在可选配置 `content_position` / `content_index`，用于在命中的 message 内继续定位 block；后台 provider 配置页也同步提供了对应选择方式。

#### 变更

- Cache breakpoint 匹配前会先规范化 Claude shorthand 内容，因此 `system: "..."`、`messages[*].content: "..."` 这类简写形式现在会和标准 text block 使用同一套索引规则。
- built-in Claude 渠道在未显式指定 TTL 的 `cache_control` 场景下，现统一按 `5m` 处理缓存亲和相关行为与示例说明。

#### 修复

- 修复 Codex 凭证健康状态判定：上游返回 `402 deactivated_workspace` 时，普通请求与 `v1/usage` 现在都会将该凭证识别为 dead，而不再按瞬时失败重试。
- 修复 OpenAI `responses -> chat completions` 流式转换在 `response.function_call_arguments.done` 缺少 `name` 字段时的解码失败问题，避免出现 `failed to decode response_stream_json ... missing field 'name'` 这类报错。

#### 兼容性

- 无需执行存储迁移。
- 现有 Claude `cache_breakpoints` 配置保持兼容；`content_position` / `content_index` 只是可选增强项。
- 当 Codex 上游明确返回 `402 deactivated_workspace` 时，对应凭证现在会自动转为 `dead`。

## v0.3.35

### English

#### Added

- Added an optional `priority_tier` field to Codex credentials so admins can force Codex upstream requests to use `service_tier=priority` per credential.

#### Changed

- Refined Codex priority-tier override behavior: `service_tier` is now forced to `priority` only when the credential-level `priority_tier` flag is explicitly enabled; otherwise the request's original tier is preserved.

#### Fixed

- Fixed OpenAI `chat completions -> responses` history conversion so assistant output-message IDs now use the required `msg_*` format, preventing Codex upstream `400 invalid_request_error` responses such as `Invalid 'input[1].id': 'assistant_1'`.
- Fixed the same assistant output-message ID format in Claude -> OpenAI Responses conversion so historical assistant messages stay compatible with upstream Responses validation.

#### Compatibility

- No storage migration is required.
- Existing Codex credentials remain unaffected until `priority_tier` is explicitly enabled.

### 中文

#### 新增

- Codex 凭证新增可选字段 `priority_tier`，管理员可按凭证维度强制上游请求使用 `service_tier=priority`。

#### 变更

- 调整 Codex 的 priority tier 覆盖逻辑：只有在凭证级 `priority_tier` 显式开启时才会强制写入 `service_tier=priority`；未开启时保留请求原始的 tier 配置。

#### 修复

- 修复 OpenAI `chat completions -> responses` 历史消息转换逻辑：assistant 输出消息 ID 现统一使用上游要求的 `msg_*` 格式，避免 Codex 上游返回 `Invalid 'input[1].id': 'assistant_1'` 这类 `400 invalid_request_error`。
- 修复 Claude -> OpenAI Responses 转换中的同类 assistant 输出消息 ID 格式问题，确保历史 assistant 消息同样兼容上游 Responses 校验。

#### 兼容性

- 无需执行存储迁移。
- 现有 Codex 凭证默认不会改变行为；仅在显式开启 `priority_tier` 后才会启用覆盖逻辑。


## v0.3.34

### English

#### Fixed

- Fixed ClaudeCode OAuth persistence so unlabeled OAuth credentials no longer create duplicate rows that can later fail `v1/usage` with `no eligible credential`.
- Fixed admin `Test All Credentials` for browser-driven sessions by dropping browser-only passthrough headers such as `origin`, `referer`, `cookie`, `sec-fetch-*`, and `sec-ch-ua*` before forwarding upstream requests.
- Fixed default credential naming during persistence so OAuth credentials prefer `user_email`/`client_email` when available instead of falling back to numeric placeholder names.

#### Changed

- Updated the release validation command to use workspace-wide clippy checks with `-D warnings` and `-A clippy::too_many_arguments`, matching the repository's current release expectations.

#### Compatibility

- No storage migration is required.
- Existing duplicated historical credentials are not removed automatically; they can be cleaned up from the admin credential list if needed.

### 中文

#### 修复

- 修复 ClaudeCode OAuth 持久化逻辑：未命名的 OAuth 凭证不再重复落库，避免后续 `v1/usage` 出现 `no eligible credential` 的异常。
- 修复后台 `测试所有凭证` 在浏览器场景下误伤 ClaudeCode 凭证的问题：转发上游请求前会过滤 `origin`、`referer`、`cookie`、`sec-fetch-*`、`sec-ch-ua*` 等浏览器专用请求头。
- 修复凭证默认命名逻辑：持久化时若可用，优先使用 `user_email` / `client_email` 作为默认名称，不再优先落成数字占位名。

#### 变更

- 发版脚本中的校验命令已调整为工作区级别的 clippy 检查，并使用 `-D warnings -A clippy::too_many_arguments`，与当前仓库发版要求保持一致。

#### 兼容性

- 无需执行存储迁移。
- 旧版本已经产生的重复历史凭证不会自动删除，如有需要可在后台凭证列表中手动清理。

## v0.3.33

### English

#### Added

- Added a `Test All Credentials` action in the admin credential workspace. It validates each credential by requesting the model list, picking the first available model, and sending a lightweight test chat request.

#### Changed

- Renamed the admin provider workspace entry from `渠道` to `渠道/凭证`, and renamed the provider config tab label to `渠道类型` to better distinguish channel type from credential management.
- Updated the bulk-delete copy in the credential workspace from `删除全部 dead` to `删除全部不可用凭证`.
- Provider-scoped OpenAI-compatible `v1/models` and `v1/chat/completions` routes now accept an optional `credential_id` selector so admin-side verification can target a specific credential directly.

#### Fixed

- Batch credential testing no longer marks transient upstream failures as dead immediately. `429/500/502/503/504` are now recorded as `partial` instead of `dead`.
- Codex `403` HTML edge pages, including common Cloudflare block/interstitial responses, no longer mark credentials as dead directly and are now treated as transient failures.

#### Compatibility

- No storage migration is required.
- Existing dead/partial credential state semantics remain unchanged; this update only makes admin-side verification and Codex edge-failure handling less aggressive.

### 中文

#### 新增

- 后台凭证工作区新增 `测试所有凭证` 按钮。该按钮会先请求模型列表，取第一个可用模型，再发送一条轻量测试消息，用于逐个校验凭证可用性。

#### 变更

- 后台 provider 工作区入口文案由 `渠道` 调整为 `渠道/凭证`，provider 配置页中的下拉项文案由 `渠道` 调整为 `渠道类型`，避免把渠道类型和凭证管理混在一起。
- 凭证工作区的批量删除文案由 `删除全部 dead` 调整为 `删除全部不可用凭证`。
- provider 作用域下的 OpenAI 兼容 `v1/models` 与 `v1/chat/completions` 路由新增可选 `credential_id` 定向参数，方便后台逐个校验凭证。

#### 修复

- 批量测试凭证时，不再把瞬时上游故障直接标记为 dead；`429/500/502/503/504` 现在会记录为 `partial`。
- 修复 Codex `403` HTML 边缘页（包括常见的 Cloudflare 拦截/过渡页）被直接判定为 dead 的问题，现统一按瞬时失败处理。

#### 兼容性

- 无需执行存储迁移。
- 已有 dead/partial 凭证状态语义保持不变；本次更新仅让后台校验与 Codex 边缘故障处理更温和。
